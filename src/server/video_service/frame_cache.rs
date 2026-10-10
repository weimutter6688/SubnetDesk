use super::*;

pub(super) struct CachedPixels {
    data: Vec<u8>,
    width: usize,
    height: usize,
    stride: Vec<usize>,
    format: scrap::Pixfmt,
}

impl CachedPixels {
    #[cfg(windows)]
    fn empty() -> Self {
        Self { data: Vec::new(), width: 0, height: 0, stride: Vec::new(), format: scrap::Pixfmt::BGRA }
    }
}

impl TraitPixelBuffer for CachedPixels {
    fn data(&self) -> &[u8] { &self.data }
    fn width(&self) -> usize { self.width }
    fn height(&self) -> usize { self.height }
    fn stride(&self) -> Vec<usize> { self.stride.clone() }
    fn pixfmt(&self) -> scrap::Pixfmt { self.format }
}

pub(super) enum CachedFrame {
    Pixels(Arc<CachedPixels>),
    #[cfg(windows)]
    Texture(scrap::dxgi::CachedTexture, usize),
}

#[derive(Default)]
pub(super) struct FrameCache {
    pub generation: u64,
    pub frame: Option<CachedFrame>,
    #[cfg(windows)]
    readback: Option<Arc<CachedPixels>>,
    #[cfg(windows)]
    readback_generation: u64,
}

impl FrameCache {
    pub fn clear(&mut self) {
        // Pixel workers may still remember a preceding generation. Reusing an
        // old number after switching capture input would reuse their old YUV.
        let generation = self.generation;
        *self = Self::default();
        self.generation = generation;
    }

    pub fn update(&mut self, frame: &scrap::Frame<'_>) -> ResultType<()> {
        match frame {
            scrap::Frame::PixelBuffer(pixels) => self.update_pixels(pixels),
            scrap::Frame::Texture((texture, rotation)) => {
                #[cfg(windows)]
                {
                    if !matches!(self.frame, Some(CachedFrame::Texture(_, _))) {
                        // SAFETY: the capturer owns this live texture until its next
                        // frame call. The cache is scoped to that same capturer.
                        let cached = unsafe { scrap::dxgi::CachedTexture::new(*texture)? };
                        self.frame = Some(CachedFrame::Texture(cached, *rotation));
                    }
                    if let Some(CachedFrame::Texture(cached, saved_rotation)) = &mut self.frame {
                        // SAFETY: capture dimensions/device stay fixed for this run.
                        unsafe { cached.copy_from(*texture) };
                        *saved_rotation = *rotation;
                    }
                    self.generation = self.generation.wrapping_add(1);
                }
                #[cfg(not(windows))]
                {
                    let _ = (texture, rotation);
                    bail!("Texture frame caching is only supported on Windows");
                }
            }
        }
        Ok(())
    }

    pub fn update_pixels(&mut self, pixels: &dyn TraitPixelBuffer) {
        // A worker may still be reading the previous snapshot. Never overwrite
        // it or clone its old full-frame contents just to replace them.
        let reusable = match &mut self.frame {
            Some(CachedFrame::Pixels(cached)) => Arc::get_mut(cached),
            _ => None,
        };
        if let Some(cached) = reusable {
            cached.data.resize(pixels.data().len(), 0);
            cached.data.copy_from_slice(pixels.data());
            cached.width = pixels.width();
            cached.height = pixels.height();
            cached.stride = pixels.stride();
            cached.format = pixels.pixfmt();
        } else {
            self.frame = Some(CachedFrame::Pixels(Arc::new(CachedPixels {
                data: pixels.data().to_vec(), width: pixels.width(), height: pixels.height(),
                stride: pixels.stride(), format: pixels.pixfmt(),
            })));
        }
        self.generation = self.generation.wrapping_add(1);
    }

    pub fn ensure_pixels(&mut self) -> ResultType<()> {
        #[cfg(windows)]
        if let Some(CachedFrame::Texture(texture, rotation)) = &mut self.frame {
            if *rotation != 0 { bail!("Cannot read back an unrotated desktop texture"); }
            if self.readback_generation != self.generation || self.readback.is_none() {
                let reusable = self.readback.as_mut().and_then(Arc::get_mut);
                if let Some(pixels) = reusable {
                    let (w, h, stride) = texture.readback(&mut pixels.data)?;
                    pixels.width = w;
                    pixels.height = h;
                    pixels.stride.clear();
                    pixels.stride.push(stride);
                } else {
                    let mut pixels = CachedPixels::empty();
                    let (w, h, stride) = texture.readback(&mut pixels.data)?;
                    pixels.width = w;
                    pixels.height = h;
                    pixels.stride.push(stride);
                    self.readback = Some(Arc::new(pixels));
                }
                self.readback_generation = self.generation;
            }
        }
        Ok(())
    }

    pub fn pixels(&self) -> Option<&Arc<CachedPixels>> {
        match &self.frame {
            Some(CachedFrame::Pixels(pixels)) => Some(pixels),
            #[cfg(windows)]
            Some(CachedFrame::Texture(_, _)) if self.readback_generation == self.generation => self.readback.as_ref(),
            _ => None,
        }
    }
}
