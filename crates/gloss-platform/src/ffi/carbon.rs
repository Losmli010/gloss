//! `Carbon`（`HIToolbox`）：本层用到的系统输入态查询。

use super::cf::Boolean;

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn IsSecureEventInputEnabled() -> Boolean;
}

pub(crate) fn is_secure_event_input_enabled() -> bool {
    // SAFETY: 纯查询型 FFI，无参数、无前置条件、不涉及内存所有权。
    let enabled = unsafe { IsSecureEventInputEnabled() };
    enabled != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secure_input_query_links_and_answers_consistently() {
        let first = is_secure_event_input_enabled();
        assert_eq!(
            first,
            is_secure_event_input_enabled(),
            "the query is a pure read of a system-wide flag, so two calls in a row must agree"
        );
    }
}
