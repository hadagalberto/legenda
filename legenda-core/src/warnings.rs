//! Non-fatal diagnostics collected by library operations.
//!
//! Operations that recover from bad input (for example a shift that would move
//! a cue before the start of the timeline, or a legacy byte inside an
//! otherwise UTF-8 subtitle file) must not fail the whole job. They record a
//! warning here instead. The CLI drains the buffer after each command and
//! prints every message to stderr; library users can inspect the same buffer
//! with [`take`].

use std::cell::RefCell;
use std::fmt::Display;

thread_local! {
    static WARNINGS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Records a non-fatal diagnostic for the current thread.
pub fn warn(message: impl Display) {
    WARNINGS.with(|warnings| warnings.borrow_mut().push(message.to_string()));
}

/// Removes and returns every diagnostic recorded so far on this thread.
pub fn take() -> Vec<String> {
    WARNINGS.with(|warnings| std::mem::take(&mut *warnings.borrow_mut()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warnings_are_collected_and_drained() {
        let _ = take();
        warn("first");
        warn(format!("second {}", 2));
        assert_eq!(take(), ["first", "second 2"]);
        assert!(take().is_empty());
    }
}
