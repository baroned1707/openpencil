//! The licence allow-list. A family is only ever downloaded when BOTH the
//! Fontsource catalog and the google/fonts `METADATA.pb` name one of these.

/// Open licences whose terms permit installing the font on the user's own
/// machine, which is what registering a downloaded face does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum License {
    Ofl,
    Apache,
    Ufl,
}

impl License {
    /// Fontsource spelling (SPDX-like): `OFL-1.1`, `Apache-2.0`, `UFL-1.0`.
    pub fn from_catalog(value: &str) -> Option<Self> {
        match value.trim() {
            "OFL-1.1" => Some(Self::Ofl),
            "Apache-2.0" => Some(Self::Apache),
            "UFL-1.0" => Some(Self::Ufl),
            _ => None,
        }
    }

    /// google/fonts `METADATA.pb` spelling: `OFL`, `APACHE2`, `UFL`.
    pub fn from_metadata(value: &str) -> Option<Self> {
        match value.trim() {
            "OFL" => Some(Self::Ofl),
            "APACHE2" => Some(Self::Apache),
            "UFL" => Some(Self::Ufl),
            _ => None,
        }
    }

    /// Top-level directory in the google/fonts repository.
    pub fn directory(self) -> &'static str {
        match self {
            Self::Ofl => "ofl",
            Self::Apache => "apache",
            Self::Ufl => "ufl",
        }
    }

    /// Name of the licence text that sits next to the fonts in that directory.
    pub fn file_name(self) -> &'static str {
        match self {
            Self::Ofl => "OFL.txt",
            Self::Apache => "LICENSE.txt",
            Self::Ufl => "UFL.txt",
        }
    }

    /// Stable label stored in `source.json`.
    pub fn label(self) -> &'static str {
        match self {
            Self::Ofl => "OFL-1.1",
            Self::Apache => "Apache-2.0",
            Self::Ufl => "UFL-1.0",
        }
    }

    /// Directories to probe, in the fixed order the spec asks for.
    pub const PROBE_ORDER: [License; 3] = [Self::Ofl, Self::Apache, Self::Ufl];
}
