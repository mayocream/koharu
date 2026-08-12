use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InputEvent {
    Focus {
        focused: bool,
    },
    PointerMoved {
        x: f64,
        y: f64,
        modifiers: InputModifiers,
    },
    PointerLeft {
        x: f64,
        y: f64,
        modifiers: InputModifiers,
    },
    MouseButton {
        state: ButtonState,
        button: MouseButton,
        x: f64,
        y: f64,
        modifiers: InputModifiers,
        click_count: u8,
    },
    Scroll {
        delta: ScrollDelta,
        x: f64,
        y: f64,
        modifiers: InputModifiers,
    },
    Key {
        state: KeyState,
        windows_key_code: i32,
        platform_key_code: i32,
        character: u16,
        unmodified_character: u16,
        text: Vec<u16>,
        repeat: bool,
        modifiers: InputModifiers,
        location: KeyLocation,
    },
    ImePreedit {
        text: Box<str>,
        cursor: Option<(usize, usize)>,
    },
    ImeCommit {
        text: Box<str>,
    },
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct InputModifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub meta: bool,
    pub left_mouse: bool,
    pub middle_mouse: bool,
    pub right_mouse: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyLocation {
    Left,
    Right,
    Numpad,
    #[default]
    Standard,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ButtonState {
    Pressed,
    Released,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyState {
    Pressed,
    Released,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Back,
    Forward,
    Other(u16),
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "unit", rename_all = "snake_case")]
pub enum ScrollDelta {
    Lines { x: f32, y: f32 },
    Pixels { x: f64, y: f64 },
}
