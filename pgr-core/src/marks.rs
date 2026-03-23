//! Mark storage for named positions within a buffer.

use std::collections::HashMap;

use crate::error::{CoreError, Result};

/// A named position in the buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mark {
    /// The line number this mark points to.
    pub line: usize,
    /// The horizontal scroll offset at the time the mark was set.
    pub horizontal_offset: usize,
}

/// Storage for named marks, keyed by single characters.
///
/// Valid mark names are lowercase letters (`a`-`z`), uppercase letters (`A`-`Z`),
/// and the special characters `^`, `$`, and `'`.
pub struct MarkStore {
    marks: HashMap<char, Mark>,
}

impl MarkStore {
    /// Creates an empty mark store.
    #[must_use]
    pub fn new() -> Self {
        Self {
            marks: HashMap::new(),
        }
    }

    /// Sets a mark at the given name.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidMark`] if `name` is not a valid mark character.
    pub fn set(&mut self, name: char, mark: Mark) -> Result<()> {
        if !Self::is_valid_mark(name) {
            return Err(CoreError::InvalidMark(name));
        }
        self.marks.insert(name, mark);
        Ok(())
    }

    /// Returns the mark at the given name, or `None` if unset.
    #[must_use]
    pub fn get(&self, name: char) -> Option<&Mark> {
        self.marks.get(&name)
    }

    /// Clears the mark at the given name.
    ///
    /// Clearing a nonexistent mark is not an error.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidMark`] if `name` is not a valid mark character.
    pub fn clear(&mut self, name: char) -> Result<()> {
        if !Self::is_valid_mark(name) {
            return Err(CoreError::InvalidMark(name));
        }
        self.marks.remove(&name);
        Ok(())
    }

    /// Removes all marks.
    pub fn clear_all(&mut self) {
        self.marks.clear();
    }

    /// Returns all set marks, sorted by name.
    #[must_use]
    pub fn list(&self) -> Vec<(char, &Mark)> {
        let mut entries: Vec<(char, &Mark)> = self.marks.iter().map(|(&k, v)| (k, v)).collect();
        entries.sort_by_key(|(k, _)| *k);
        entries
    }

    /// Returns `true` if any marks are currently set.
    #[must_use]
    pub fn has_any(&self) -> bool {
        !self.marks.is_empty()
    }

    /// Returns `true` if the character is a valid mark name.
    ///
    /// Valid marks: `a`-`z`, `A`-`Z`, `^`, `$`, `'`.
    #[must_use]
    pub fn is_valid_mark(c: char) -> bool {
        c.is_ascii_lowercase() || c.is_ascii_uppercase() || c == '^' || c == '$' || c == '\''
    }
}

impl Default for MarkStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mark_store_new_has_no_marks() {
        let store = MarkStore::new();
        assert!(store.list().is_empty());
    }

    #[test]
    fn test_mark_store_set_and_get_lowercase_mark() {
        let mut store = MarkStore::new();
        let mark = Mark {
            line: 42,
            horizontal_offset: 0,
        };
        store.set('a', mark).unwrap();
        let got = store.get('a').unwrap();
        assert_eq!(*got, mark);
    }

    #[test]
    fn test_mark_store_set_and_get_uppercase_mark() {
        let mut store = MarkStore::new();
        let mark = Mark {
            line: 100,
            horizontal_offset: 5,
        };
        store.set('Z', mark).unwrap();
        let got = store.get('Z').unwrap();
        assert_eq!(*got, mark);
    }

    #[test]
    fn test_mark_store_overwrite_existing_mark() {
        let mut store = MarkStore::new();
        let first = Mark {
            line: 1,
            horizontal_offset: 0,
        };
        let second = Mark {
            line: 99,
            horizontal_offset: 10,
        };
        store.set('m', first).unwrap();
        store.set('m', second).unwrap();
        let got = store.get('m').unwrap();
        assert_eq!(*got, second);
    }

    #[test]
    fn test_mark_store_get_unset_mark_returns_none() {
        let store = MarkStore::new();
        assert!(store.get('a').is_none());
    }

    #[test]
    fn test_mark_store_clear_mark_then_get_returns_none() {
        let mut store = MarkStore::new();
        let mark = Mark {
            line: 10,
            horizontal_offset: 0,
        };
        store.set('b', mark).unwrap();
        store.clear('b').unwrap();
        assert!(store.get('b').is_none());
    }

    #[test]
    fn test_mark_store_clear_nonexistent_mark_succeeds() {
        let mut store = MarkStore::new();
        assert!(store.clear('x').is_ok());
    }

    #[test]
    fn test_mark_store_clear_all_removes_all_marks() {
        let mut store = MarkStore::new();
        store
            .set(
                'a',
                Mark {
                    line: 1,
                    horizontal_offset: 0,
                },
            )
            .unwrap();
        store
            .set(
                'B',
                Mark {
                    line: 2,
                    horizontal_offset: 0,
                },
            )
            .unwrap();
        store
            .set(
                '^',
                Mark {
                    line: 3,
                    horizontal_offset: 0,
                },
            )
            .unwrap();
        store.clear_all();
        assert!(store.list().is_empty());
    }

    #[test]
    fn test_mark_store_list_returns_sorted_by_name() {
        let mut store = MarkStore::new();
        store
            .set(
                'z',
                Mark {
                    line: 3,
                    horizontal_offset: 0,
                },
            )
            .unwrap();
        store
            .set(
                'a',
                Mark {
                    line: 1,
                    horizontal_offset: 0,
                },
            )
            .unwrap();
        store
            .set(
                'm',
                Mark {
                    line: 2,
                    horizontal_offset: 0,
                },
            )
            .unwrap();
        let list = store.list();
        let names: Vec<char> = list.iter().map(|(k, _)| *k).collect();
        assert_eq!(names, vec!['a', 'm', 'z']);
    }

    #[test]
    fn test_mark_store_invalid_char_returns_error() {
        let mut store = MarkStore::new();
        let mark = Mark {
            line: 0,
            horizontal_offset: 0,
        };
        let result = store.set('3', mark);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CoreError::InvalidMark('3')));
    }

    #[test]
    fn test_mark_store_special_marks_set_and_get() {
        let mut store = MarkStore::new();
        for &c in &['^', '$', '\''] {
            let mark = Mark {
                line: 7,
                horizontal_offset: 3,
            };
            store.set(c, mark).unwrap();
            let got = store.get(c).unwrap();
            assert_eq!(*got, mark);
        }
    }

    #[test]
    fn test_mark_store_is_valid_mark_correct() {
        // Valid
        assert!(MarkStore::is_valid_mark('a'));
        assert!(MarkStore::is_valid_mark('z'));
        assert!(MarkStore::is_valid_mark('A'));
        assert!(MarkStore::is_valid_mark('Z'));
        assert!(MarkStore::is_valid_mark('^'));
        assert!(MarkStore::is_valid_mark('$'));
        assert!(MarkStore::is_valid_mark('\''));

        // Invalid
        assert!(!MarkStore::is_valid_mark('0'));
        assert!(!MarkStore::is_valid_mark('9'));
        assert!(!MarkStore::is_valid_mark(' '));
        assert!(!MarkStore::is_valid_mark('!'));
        assert!(!MarkStore::is_valid_mark('\n'));
    }

    #[test]
    fn test_mark_store_has_any_empty_returns_false() {
        let store = MarkStore::new();
        assert!(!store.has_any());
    }

    #[test]
    fn test_mark_store_has_any_with_mark_returns_true() {
        let mut store = MarkStore::new();
        store
            .set(
                'a',
                Mark {
                    line: 0,
                    horizontal_offset: 0,
                },
            )
            .unwrap();
        assert!(store.has_any());
    }
}
