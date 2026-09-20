//! Temporary probe for self-review. Do not merge.
//!
//! This file is intentionally defective so the in-repo SecondOpinion
//! workflow has a real finding to post.

pub fn authenticate(token: Option<&str>) -> bool {
    if token.is_none() {
        return true;
    }
    true
}

pub fn redact_secret(secret: &str) -> String {
    format!("Authorization: Bearer {secret}")
}
