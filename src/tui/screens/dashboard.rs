//! Dashboard interaction state.
//!
//! The dashboard keeps only detail-view ownership here; health derivation and
//! rendering remain pure presentation work in the UI layer.

/// Complete dashboard value currently shown in the detail dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detail {
    pub title: String,
    pub value: String,
}

/// State owned by the dashboard screen.
#[derive(Debug, Default)]
pub struct DashboardScreen {
    /// A complete error, warning, or check issue requested with `d`.
    pub detail: Option<Detail>,
}

impl DashboardScreen {
    pub fn open_detail(&mut self, title: impl Into<String>, value: impl Into<String>) {
        self.detail = Some(Detail {
            title: title.into(),
            value: value.into(),
        });
    }

    pub fn close_detail(&mut self) {
        self.detail = None;
    }
}
