use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Preset {
    EvenHorizontal,
    EvenVertical,
    MainHorizontal,
    MainVertical,
    Tiled,
}

impl Preset {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::EvenHorizontal => "even-horizontal",
            Self::EvenVertical => "even-vertical",
            Self::MainHorizontal => "main-horizontal",
            Self::MainVertical => "main-vertical",
            Self::Tiled => "tiled",
        }
    }

    pub(crate) const ALL: [Self; 5] = [
        Self::EvenHorizontal,
        Self::EvenVertical,
        Self::MainHorizontal,
        Self::MainVertical,
        Self::Tiled,
    ];
}
