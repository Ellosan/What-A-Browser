//! The address field's editing state.

/// A single-line text field with a caret and a select-all state.
#[derive(Clone, Debug, Default)]
pub struct Omnibox {
    text: String,
    /// Caret position, as a character index.
    caret: usize,
    focused: bool,
    /// True when focusing selected everything, so the next keystroke replaces it.
    all_selected: bool,
    /// The URL to show when the field is not being edited.
    display_url: String,
}

impl Omnibox {
    pub fn new() -> Self {
        Omnibox::default()
    }

    /// The text the user sees: what they are typing, or the current URL.
    pub fn visible_text(&self) -> &str {
        if self.focused {
            &self.text
        } else {
            &self.display_url
        }
    }

    /// The text being edited.
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn is_focused(&self) -> bool {
        self.focused
    }

    pub fn all_selected(&self) -> bool {
        self.all_selected
    }

    pub fn caret(&self) -> usize {
        self.caret
    }

    /// Sets the URL shown while not editing.
    pub fn set_url(&mut self, url: impl Into<String>) {
        self.display_url = url.into();
        if !self.focused {
            self.text = self.display_url.clone();
            self.caret = self.text.chars().count();
        }
    }

    /// Focuses the field, selecting everything as browsers do.
    pub fn focus(&mut self) {
        self.focused = true;
        self.text = self.display_url.clone();
        self.caret = self.text.chars().count();
        self.all_selected = true;
    }

    /// Gives up focus and discards any edit.
    pub fn blur(&mut self) {
        self.focused = false;
        self.all_selected = false;
        self.text = self.display_url.clone();
        self.caret = self.text.chars().count();
    }

    /// Inserts typed text.
    pub fn insert(&mut self, input: &str) {
        if !self.focused {
            return;
        }
        if self.all_selected {
            self.text.clear();
            self.caret = 0;
            self.all_selected = false;
        }
        let byte = self.byte_offset(self.caret);
        self.text.insert_str(byte, input);
        self.caret += input.chars().count();
    }

    /// Deletes backwards from the caret.
    pub fn backspace(&mut self) {
        if !self.focused {
            return;
        }
        if self.all_selected {
            self.text.clear();
            self.caret = 0;
            self.all_selected = false;
            return;
        }
        if self.caret == 0 {
            return;
        }
        let end = self.byte_offset(self.caret);
        let start = self.byte_offset(self.caret - 1);
        self.text.replace_range(start..end, "");
        self.caret -= 1;
    }

    /// Deletes forwards from the caret.
    pub fn delete(&mut self) {
        if !self.focused {
            return;
        }
        if self.all_selected {
            self.text.clear();
            self.caret = 0;
            self.all_selected = false;
            return;
        }
        let length = self.text.chars().count();
        if self.caret >= length {
            return;
        }
        let start = self.byte_offset(self.caret);
        let end = self.byte_offset(self.caret + 1);
        self.text.replace_range(start..end, "");
    }

    /// Clears the field, leaving it focused.
    pub fn clear(&mut self) {
        self.text.clear();
        self.caret = 0;
        self.all_selected = false;
    }

    pub fn move_left(&mut self) {
        self.all_selected = false;
        self.caret = self.caret.saturating_sub(1);
    }

    pub fn move_right(&mut self) {
        self.all_selected = false;
        self.caret = (self.caret + 1).min(self.text.chars().count());
    }

    pub fn move_home(&mut self) {
        self.all_selected = false;
        self.caret = 0;
    }

    pub fn move_end(&mut self) {
        self.all_selected = false;
        self.caret = self.text.chars().count();
    }

    pub fn select_all(&mut self) {
        self.all_selected = true;
        self.caret = self.text.chars().count();
    }

    /// Deletes the word before the caret.
    pub fn delete_word(&mut self) {
        if !self.focused || self.caret == 0 {
            return;
        }
        if self.all_selected {
            self.clear();
            return;
        }
        let chars: Vec<char> = self.text.chars().collect();
        let mut index = self.caret;
        while index > 0 && chars[index - 1].is_whitespace() {
            index -= 1;
        }
        while index > 0 && !chars[index - 1].is_whitespace() {
            index -= 1;
        }
        let start = self.byte_offset(index);
        let end = self.byte_offset(self.caret);
        self.text.replace_range(start..end, "");
        self.caret = index;
    }

    /// Accepts the edit, returning the text to navigate to.
    pub fn commit(&mut self) -> String {
        let text = self.text.trim().to_string();
        self.focused = false;
        self.all_selected = false;
        text
    }

    /// Byte offset of a character index.
    fn byte_offset(&self, char_index: usize) -> usize {
        self.text
            .char_indices()
            .nth(char_index)
            .map(|(offset, _)| offset)
            .unwrap_or(self.text.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn focused(text: &str) -> Omnibox {
        let mut omnibox = Omnibox::new();
        omnibox.set_url(text);
        omnibox.focus();
        omnibox
    }

    #[test]
    fn shows_the_url_when_not_focused() {
        let mut omnibox = Omnibox::new();
        omnibox.set_url("https://example.com/");
        assert!(!omnibox.is_focused());
        assert_eq!(omnibox.visible_text(), "https://example.com/");
    }

    #[test]
    fn focusing_selects_everything() {
        let omnibox = focused("https://example.com/");
        assert!(omnibox.is_focused());
        assert!(omnibox.all_selected());
        assert_eq!(omnibox.text(), "https://example.com/");
    }

    #[test]
    fn typing_replaces_a_full_selection() {
        let mut omnibox = focused("https://example.com/");
        omnibox.insert("n");
        assert_eq!(omnibox.text(), "n");
        assert!(!omnibox.all_selected());
        omnibox.insert("ew");
        assert_eq!(omnibox.text(), "new");
        assert_eq!(omnibox.caret(), 3);
    }

    #[test]
    fn typing_when_unfocused_is_ignored() {
        let mut omnibox = Omnibox::new();
        omnibox.set_url("https://a/");
        omnibox.insert("x");
        assert_eq!(omnibox.text(), "https://a/");
    }

    #[test]
    fn backspace_deletes_the_selection_then_characters() {
        let mut omnibox = focused("abc");
        omnibox.backspace();
        assert_eq!(
            omnibox.text(),
            "",
            "the first backspace clears the selection"
        );
        omnibox.insert("xy");
        omnibox.backspace();
        assert_eq!(omnibox.text(), "x");
        omnibox.backspace();
        assert_eq!(omnibox.text(), "");
        omnibox.backspace();
        assert_eq!(omnibox.text(), "", "backspace at the start is harmless");
    }

    #[test]
    fn caret_movement_and_mid_string_editing() {
        let mut omnibox = focused("abcd");
        omnibox.move_home();
        assert_eq!(omnibox.caret(), 0);
        omnibox.move_right();
        omnibox.insert("X");
        assert_eq!(omnibox.text(), "aXbcd");
        omnibox.move_end();
        assert_eq!(omnibox.caret(), 5);
        omnibox.move_left();
        omnibox.backspace();
        assert_eq!(omnibox.text(), "aXbd");
    }

    #[test]
    fn caret_movement_is_clamped() {
        let mut omnibox = focused("ab");
        omnibox.move_home();
        omnibox.move_left();
        assert_eq!(omnibox.caret(), 0);
        omnibox.move_end();
        omnibox.move_right();
        assert_eq!(omnibox.caret(), 2);
    }

    #[test]
    fn forward_delete() {
        let mut omnibox = focused("abc");
        omnibox.move_home();
        omnibox.delete();
        assert_eq!(omnibox.text(), "bc");
        omnibox.move_end();
        omnibox.delete();
        assert_eq!(omnibox.text(), "bc", "delete at the end is harmless");
    }

    #[test]
    fn word_deletion() {
        let mut omnibox = focused("one two three");
        omnibox.select_all();
        omnibox.insert("one two three");
        omnibox.delete_word();
        assert_eq!(omnibox.text(), "one two ");
        omnibox.delete_word();
        assert_eq!(omnibox.text(), "one ");
    }

    #[test]
    fn commit_returns_trimmed_text_and_blurs() {
        let mut omnibox = focused("x");
        omnibox.insert("  example.com  ");
        assert_eq!(omnibox.commit(), "example.com");
        assert!(!omnibox.is_focused());
    }

    #[test]
    fn blurring_discards_the_edit() {
        let mut omnibox = focused("https://example.com/");
        omnibox.insert("something else");
        omnibox.blur();
        assert_eq!(omnibox.visible_text(), "https://example.com/");
        assert_eq!(omnibox.text(), "https://example.com/");
    }

    #[test]
    fn a_url_change_while_focused_does_not_disturb_the_edit() {
        let mut omnibox = focused("https://a/");
        omnibox.insert("typing");
        omnibox.set_url("https://b/");
        assert_eq!(omnibox.text(), "typing");
        omnibox.blur();
        assert_eq!(omnibox.visible_text(), "https://b/");
    }

    #[test]
    fn multibyte_text_is_edited_by_character() {
        let mut omnibox = focused("");
        omnibox.insert("héllo→");
        assert_eq!(omnibox.caret(), 6);
        omnibox.backspace();
        assert_eq!(omnibox.text(), "héllo");
        omnibox.move_home();
        omnibox.move_right();
        omnibox.delete();
        assert_eq!(omnibox.text(), "hllo");
    }
}
