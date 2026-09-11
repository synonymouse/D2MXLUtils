//! Game-create autofill control and its sole-use synthetic input.

mod hotkey;
mod input;

pub(crate) use hotkey::{
    update_game_create_autofill_hotkey, GameCreateAutofillConfig, GameCreateAutofillHotkeyState,
    __cmd__update_game_create_autofill_hotkey,
};
