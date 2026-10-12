#![deny(warnings, clippy::all)]

#[cfg(target_os = "linux")]
use super::uinput::{
    IPC_CONN_TIMEOUT, IPC_POSTFIX_CONTROL, IPC_POSTFIX_KEYBOARD, IPC_POSTFIX_MOUSE,
};
use crate::ipc::input_client::InputIpcClient;
#[cfg(target_os = "linux")]
use crate::ipc::input_client::REQUEST_TIMEOUT;
use crate::ipc::{self, Data, DataKeyboard, DataMouse};
use enigo::{Key, KeyboardControllable, MouseButton, MouseControllable};
#[cfg(any(target_os = "linux", test))]
use hbb_common::tokio;
use hbb_common::{bail, log, ResultType};

pub struct UInputKeyboard {
    ipc: InputIpcClient,
}

impl UInputKeyboard {
    #[cfg(target_os = "linux")]
    pub async fn new() -> ResultType<Self> {
        let conn = ipc::connect(IPC_CONN_TIMEOUT, IPC_POSTFIX_KEYBOARD).await?;
        Ok(Self {
            ipc: InputIpcClient::new(conn),
        })
    }

    fn send(&mut self, data: Data) -> ResultType<()> {
        self.ipc.send(data)
    }
    fn send_without_result(&mut self, data: Data) {
        if let Err(err) = self.send(data) {
            log::warn!("Input IPC send failed: {err}");
        }
    }

    fn send_get_key_state(&mut self, data: Data) -> ResultType<bool> {
        match self.ipc.request(data, true) {
            Ok(Some(Data::KeyboardResponse(ipc::DataKeyboardResponse::GetKeyState(state)))) => {
                Ok(state)
            }
            Ok(Some(resp)) => {
                // FATAL error!!!
                bail!(
                    "FATAL error, wait keyboard result other response: {:?}",
                    &resp
                );
            }
            Ok(None) => {
                // FATAL error!!!
                // Maybe wait later
                bail!("FATAL error, wait keyboard result, receive None",);
            }
            Err(e) => {
                // FATAL error!!!
                bail!("Failed to read keyboard state: {}", &e);
            }
        }
    }
}

impl KeyboardControllable for UInputKeyboard {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_mut_any(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn get_key_state(&mut self, key: Key) -> bool {
        match self.send_get_key_state(Data::Keyboard(DataKeyboard::GetKeyState(key))) {
            Ok(state) => state,
            Err(e) => {
                // unreachable!()
                log::error!("Failed to get key state {}", &e);
                false
            }
        }
    }

    fn key_sequence(&mut self, sequence: &str) {
        // Sequence events are normally handled in the --server process before reaching here.
        // Forward via IPC as a fallback — input_text_wayland can still handle ASCII chars
        // via keysym/uinput, though non-ASCII will be skipped (no clipboard in --service).
        log::debug!(
            "UInputKeyboard::key_sequence called (len={})",
            sequence.len()
        );
        self.send_without_result(Data::Keyboard(DataKeyboard::Sequence(sequence.to_string())));
    }

    fn key_down(&mut self, key: Key) -> enigo::ResultType {
        self.send(Data::Keyboard(DataKeyboard::KeyDown(key)))
            .map_err(|err| err.into())
    }
    fn key_up(&mut self, key: Key) {
        self.send_without_result(Data::Keyboard(DataKeyboard::KeyUp(key)));
    }
    fn key_click(&mut self, key: Key) {
        self.send_without_result(Data::Keyboard(DataKeyboard::KeyClick(key)));
    }
}

pub struct UInputMouse {
    ipc: InputIpcClient,
}

impl UInputMouse {
    #[cfg(target_os = "linux")]
    pub async fn new() -> ResultType<Self> {
        let conn = ipc::connect(IPC_CONN_TIMEOUT, IPC_POSTFIX_MOUSE).await?;
        Ok(Self {
            ipc: InputIpcClient::new(conn),
        })
    }

    fn send(&mut self, data: Data) -> ResultType<()> {
        self.ipc.send(data)
    }
    fn send_without_result(&mut self, data: Data) {
        if let Err(err) = self.send(data) {
            log::warn!("Input IPC send failed: {err}");
        }
    }

    pub fn send_refresh(&mut self) -> ResultType<()> {
        self.send(Data::Mouse(DataMouse::Refresh))
    }
}

impl MouseControllable for UInputMouse {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_mut_any(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn mouse_move_to(&mut self, x: i32, y: i32) {
        self.send_without_result(Data::Mouse(DataMouse::MoveTo(x, y)));
    }
    fn mouse_move_relative(&mut self, x: i32, y: i32) {
        self.send_without_result(Data::Mouse(DataMouse::MoveRelative(x, y)));
    }
    fn mouse_down(&mut self, button: MouseButton) -> enigo::ResultType {
        self.send(Data::Mouse(DataMouse::Down(button)))
            .map_err(|err| err.into())
    }
    fn mouse_up(&mut self, button: MouseButton) {
        self.send_without_result(Data::Mouse(DataMouse::Up(button)));
    }
    fn mouse_click(&mut self, button: MouseButton) {
        self.send_without_result(Data::Mouse(DataMouse::Click(button)));
    }
    fn mouse_scroll_x(&mut self, length: i32) {
        self.send_without_result(Data::Mouse(DataMouse::ScrollX(length)));
    }
    fn mouse_scroll_y(&mut self, length: i32) {
        self.send_without_result(Data::Mouse(DataMouse::ScrollY(length)));
    }
}

#[cfg(target_os = "linux")]
pub async fn set_resolution(minx: i32, maxx: i32, miny: i32, maxy: i32) -> ResultType<()> {
    let mut conn = ipc::connect(IPC_CONN_TIMEOUT, IPC_POSTFIX_CONTROL).await?;
    tokio::time::timeout(REQUEST_TIMEOUT, async {
        conn.send(&Data::Control(ipc::DataControl::Resolution {
            minx,
            maxx,
            miny,
            maxy,
        }))
        .await?;
        conn.next()
            .await?
            .ok_or_else(|| hbb_common::anyhow::anyhow!("Invalid uinput resolution response"))?;
        Ok::<_, hbb_common::anyhow::Error>(())
    })
    .await??;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn key_and_mouse_down_report_transport_failure_through_enigo() {
        let (keyboard, keyboard_peer) = tokio::io::duplex(64);
        let (mouse, mouse_peer) = tokio::io::duplex(64);
        drop((keyboard_peer, mouse_peer));
        let mut keyboard = UInputKeyboard {
            ipc: InputIpcClient::new(ipc::ConnectionTmpl::new(keyboard)),
        };
        let mut mouse = UInputMouse {
            ipc: InputIpcClient::new(ipc::ConnectionTmpl::new(mouse)),
        };
        tokio::task::spawn_blocking(move || {
            assert!(KeyboardControllable::key_down(&mut keyboard, Key::Shift).is_err());
            assert!(MouseControllable::mouse_down(&mut mouse, MouseButton::Left).is_err());
            assert!(mouse.send_refresh().is_err());
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn successful_key_and_mouse_down_keep_the_existing_wire_messages() {
        let (keyboard, keyboard_peer) = tokio::io::duplex(1024);
        let (mouse, mouse_peer) = tokio::io::duplex(1024);
        let mut keyboard = UInputKeyboard {
            ipc: InputIpcClient::new(ipc::ConnectionTmpl::new(keyboard)),
        };
        let mut mouse = UInputMouse {
            ipc: InputIpcClient::new(ipc::ConnectionTmpl::new(mouse)),
        };
        let peer = tokio::spawn(async move {
            let mut keyboard = ipc::ConnectionTmpl::new(keyboard_peer);
            let mut mouse = ipc::ConnectionTmpl::new(mouse_peer);
            assert!(matches!(
                keyboard.next().await.unwrap(),
                Some(Data::Keyboard(DataKeyboard::KeyDown(Key::Shift)))
            ));
            assert!(matches!(
                mouse.next().await.unwrap(),
                Some(Data::Mouse(DataMouse::Down(MouseButton::Left)))
            ));
            assert!(matches!(
                mouse.next().await.unwrap(),
                Some(Data::Mouse(DataMouse::Refresh))
            ));
        });
        tokio::task::spawn_blocking(move || {
            keyboard.key_down(Key::Shift).unwrap();
            mouse.mouse_down(MouseButton::Left).unwrap();
            mouse.send_refresh().unwrap();
        })
        .await
        .unwrap();
        peer.await.unwrap();
    }
}
