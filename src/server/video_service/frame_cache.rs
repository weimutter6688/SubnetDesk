use super::*;

pub(super) struct CachedPixels {
    data: Vec<u8>,
    width: usize,
    height: usize,
    stride: Vec<usize>,
    format: scrap::Pixfmt,
}

impl TraitPixelBuffer for CachedPixels {
    fn data(&self) -> &[u8] { &self.data }
    fn width(&self) -> usize { self.width }
    fn height(&self) -> usize { self.height }
    fn stride(&self) -> Vec<usize> { self.stride.clone() }
    fn pixfmt(&self) -> scrap::Pixfmt { self.format }
}

pub(super) enum CachedFrame {
    Pixels(CachedPixels),
    #[cfg(windows)]
    Texture(scrap::dxgi::CachedTexture, usize),
}

#[derive(Default)]
pub(super) struct FrameCache {
    pub generation: u64,
    pub frame: Option<CachedFrame>,
}

impl FrameCache {
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
        if let Some(CachedFrame::Pixels(cached)) = &mut self.frame {
            cached.data.resize(pixels.data().len(), 0);
            cached.data.copy_from_slice(pixels.data());
            cached.width = pixels.width();
            cached.height = pixels.height();
            cached.stride = pixels.stride();
            cached.format = pixels.pixfmt();
        } else {
            self.frame = Some(CachedFrame::Pixels(CachedPixels {
                data: pixels.data().to_vec(), width: pixels.width(), height: pixels.height(),
                stride: pixels.stride(), format: pixels.pixfmt(),
            }));
        }
        self.generation = self.generation.wrapping_add(1);
    }
}
