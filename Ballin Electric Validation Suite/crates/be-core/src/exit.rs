//! Documented process exit codes for the CLI (factory automation contract).

use crate::status::TestStatus;

pub const PASS: i32 = 0;
pub const FAIL: i32 = 1;
pub const INCOMPLETE: i32 = 2;
pub const ABORTED: i32 = 3;
pub const INVALID_INPUT: i32 = 4;
pub const INTERNAL_ERROR: i32 = 5;
pub const NOT_AUTHORIZED: i32 = 6;

/// Map an aggregate run outcome to an exit code. Any failure dominates.
pub fn for_statuses<I: IntoIterator<Item = TestStatus>>(statuses: I) -> i32 {
    let mut any = false;
    let mut code = PASS;
    for s in statuses {
        any = true;
        let c = match s {
            TestStatus::Pass => PASS,
            TestStatus::Fail => return FAIL,
            TestStatus::Aborted => ABORTED,
            TestStatus::Warning | TestStatus::Skipped => PASS,
            _ => INCOMPLETE,
        };
        if c == ABORTED || (c == INCOMPLETE && code == PASS) {
            code = c;
        }
    }
    if any { code } else { INCOMPLETE }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mapping() {
        use TestStatus::*;
        assert_eq!(for_statuses([Pass, Pass]), PASS);
        assert_eq!(for_statuses([Pass, Unsupported]), INCOMPLETE);
        assert_eq!(for_statuses([Unsupported, Aborted]), ABORTED);
        assert_eq!(for_statuses([Aborted, Fail]), FAIL);
        assert_eq!(for_statuses([]), INCOMPLETE);
    }
}
