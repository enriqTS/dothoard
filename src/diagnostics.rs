//! Process-wide structured diagnostics setup.

use std::borrow::Cow;

use tracing_subscriber::EnvFilter;

pub fn init() -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .compact()
        .try_init()
        .map_err(|error| anyhow::anyhow!(error))
}

pub fn redact_remote_url(value: &str) -> Cow<'_, str> {
    let Some((scheme, remainder)) = value.split_once("://") else {
        return Cow::Borrowed(value);
    };

    let authority_end = remainder.find(['/', '?', '#']).unwrap_or(remainder.len());
    let suffix = &remainder[authority_end..];

    if remainder.contains('@') || suffix.contains(['?', '#']) {
        return Cow::Owned(format!("{scheme}://[redacted]"));
    }

    Cow::Borrowed(value)
}

pub fn redact_sensitive_text(value: &str) -> Cow<'_, str> {
    let mut output = String::with_capacity(value.len());
    let mut output_cursor = 0;
    let mut search_cursor = 0;
    let mut changed = false;

    while let Some(relative_marker) = value[search_cursor..].find("://") {
        let marker = search_cursor + relative_marker;
        let mut scheme_start = marker;

        while scheme_start > 0 && is_scheme_character(value.as_bytes()[scheme_start - 1]) {
            scheme_start -= 1;
        }

        if scheme_start == marker {
            search_cursor = marker + 3;
            continue;
        }

        let url_end = value[marker + 3..]
            .char_indices()
            .find_map(|(index, character)| is_url_boundary(character).then_some(marker + 3 + index))
            .unwrap_or(value.len());
        let candidate = &value[scheme_start..url_end];
        let redacted_candidate = redact_remote_url(candidate);

        if matches!(redacted_candidate, Cow::Owned(_)) {
            output.push_str(&value[output_cursor..scheme_start]);
            output.push_str(&redacted_candidate);
            output_cursor = url_end;
            changed = true;
        }

        search_cursor = url_end;
    }

    if changed {
        output.push_str(&value[output_cursor..]);
        Cow::Owned(output)
    } else {
        Cow::Borrowed(value)
    }
}

fn is_scheme_character(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.')
}

fn is_url_boundary(character: char) -> bool {
    character.is_whitespace()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_url_userinfo() {
        let value = "https://user:secret@example.com/repository.git";

        assert_eq!(redact_remote_url(value), "https://[redacted]");
    }

    #[test]
    fn redacts_url_query_and_fragment() {
        assert_eq!(
            redact_remote_url("https://example.com/repository.git?token=secret"),
            "https://[redacted]"
        );
        assert_eq!(
            redact_remote_url("https://example.com/repository.git#secret"),
            "https://[redacted]"
        );
    }

    #[test]
    fn preserves_urls_without_credential_components() {
        let value = "https://example.com/repository.git";

        assert_eq!(redact_remote_url(value), value);
    }

    #[test]
    fn preserves_scp_style_ssh_remote() {
        let value = "git@example.com:repository.git";

        assert_eq!(redact_remote_url(value), value);
    }

    #[test]
    fn redacts_every_sensitive_url_in_diagnostic_text() {
        let value = concat!(
            "fetch from https://example.com/public.git failed; ",
            "push to https://user:secret@example.com/private.git failed; ",
            "see https://example.com/details?token=secret"
        );

        assert_eq!(
            redact_sensitive_text(value),
            concat!(
                "fetch from https://example.com/public.git failed; ",
                "push to https://[redacted] failed; ",
                "see https://[redacted]"
            )
        );
    }

    #[test]
    fn redacts_adjacent_sensitive_urls() {
        let value = concat!(
            "https://example.com/public.git,",
            "https://user:secret@example.com/private.git"
        );

        assert_eq!(redact_sensitive_text(value), "https://[redacted]");
    }

    #[test]
    fn redacts_sensitive_query_after_url_punctuation() {
        let value = "request to https://example.com/a,b?token=secret failed";

        assert_eq!(
            redact_sensitive_text(value),
            "request to https://[redacted] failed"
        );
    }

    #[test]
    fn preserves_non_sensitive_diagnostic_text_as_borrowed() {
        let value = "fetch from https://example.com/public.git failed";

        assert!(matches!(redact_sensitive_text(value), Cow::Borrowed(_)));
    }
}
