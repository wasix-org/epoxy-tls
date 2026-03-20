//! extra tls roots added by default.
//! currently will be used to fix example.com

use rustls_pki_types::{Der, TrustAnchor};

pub const EXTRA_TLS_SERVER_ROOTS: &[TrustAnchor<'static>] = &[

];
