use serde::Deserialize;
use serde_json::Value;

pub(crate) const BRIDGE_SCRIPT: &str = r#"
(() => {
  if (window.koharu) return;
  const eventName = 'koharu:event';
  Object.defineProperty(window, 'koharu', {
    configurable: false,
    writable: false,
    value: Object.freeze({
      send(message) {
        const body = typeof message === 'string' ? message : JSON.stringify(message);
        window.ipc.postMessage(body);
      },
      listen(handler) {
        const listener = event => handler(event.detail);
        window.addEventListener(eventName, listener);
        return () => window.removeEventListener(eventName, listener);
      },
    }),
  });
})();
"#;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum IncomingMessage {
    Ready {
        dpr: f64,
        width: f64,
        height: f64,
    },
    Viewport {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        dpr: f64,
        background: [u8; 3],
    },
    Window(WindowAction),
    Application(Value),
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WindowAction {
    Drag,
    Minimize,
    ToggleMaximize,
    Close,
}

#[derive(Deserialize)]
struct Ready {
    dpr: f64,
    width: f64,
    height: f64,
}

#[derive(Deserialize)]
struct Viewport {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    dpr: f64,
    background: [u8; 3],
}

#[derive(Deserialize)]
struct WindowControl {
    action: WindowAction,
}

pub(crate) fn decode_message(bytes: &[u8]) -> Result<IncomingMessage, serde_json::Error> {
    let value = serde_json::from_slice::<Value>(bytes)?;
    match value.get("type").and_then(Value::as_str) {
        Some("ready") => {
            let message = serde_json::from_value::<Ready>(value)?;
            Ok(IncomingMessage::Ready {
                dpr: message.dpr,
                width: message.width,
                height: message.height,
            })
        }
        Some("viewport") => {
            let message = serde_json::from_value::<Viewport>(value)?;
            Ok(IncomingMessage::Viewport {
                x: message.x,
                y: message.y,
                width: message.width,
                height: message.height,
                dpr: message.dpr,
                background: message.background,
            })
        }
        Some("window") => {
            let message = serde_json::from_value::<WindowControl>(value)?;
            Ok(IncomingMessage::Window(message.action))
        }
        _ => Ok(IncomingMessage::Application(value)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserves_only_shell_messages() {
        assert_eq!(
            decode_message(br#"{"type":"viewport","x":1,"y":2,"width":3,"height":4,"dpr":2,"background":[245,245,245]}"#)
                .unwrap(),
            IncomingMessage::Viewport {
                x: 1.0,
                y: 2.0,
                width: 3.0,
                height: 4.0,
                dpr: 2.0,
                background: [245, 245, 245],
            }
        );
        assert!(matches!(
            decode_message(br#"{"type":"select","element":"abc"}"#).unwrap(),
            IncomingMessage::Application(_)
        ));
        assert_eq!(
            decode_message(br#"{"type":"window","action":"toggle_maximize"}"#).unwrap(),
            IncomingMessage::Window(WindowAction::ToggleMaximize)
        );
    }

    #[test]
    fn rejects_malformed_reserved_messages() {
        assert!(decode_message(br#"{"type":"viewport","width":"wide"}"#).is_err());
    }
}
