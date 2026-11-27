pub mod oauth;
pub mod quota;
pub mod sync;

pub(crate) const USER_AGENT: &str = concat!(
    "antigravity/hub/2.35.2 ",
    env!("CARGO_PKG_NAME"),
    "/",
    env!("CARGO_PKG_VERSION")
);
