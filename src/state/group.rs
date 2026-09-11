use uuid::Uuid;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum GroupType {
    #[default]
    Normal,
    Open,
    Isolated,
}

impl GroupType {
    #[must_use]
    pub const fn from_wire(value: i16) -> Self {
        match value {
            1 => Self::Open,
            2 => Self::Isolated,
            _ => Self::Normal,
        }
    }

    #[must_use]
    pub const fn to_wire(self) -> i16 {
        match self {
            Self::Normal => 0,
            Self::Open => 1,
            Self::Isolated => 2,
        }
    }

    #[must_use]
    pub const fn is_open(self) -> bool {
        matches!(self, Self::Open)
    }

    #[must_use]
    pub const fn is_isolated(self) -> bool {
        matches!(self, Self::Isolated)
    }
}

#[derive(Clone)]
pub struct Group {
    pub id: Uuid,
    pub name: String,
    pub password: Option<String>,
    pub persistent: bool,
    pub hidden: bool,
    pub group_type: GroupType,
}

#[cfg(test)]
mod tests {
    use super::GroupType;

    #[test]
    fn group_types_round_trip_the_protocol_values() {
        for value in 0..=2 {
            assert_eq!(GroupType::from_wire(value).to_wire(), value);
        }
        assert_eq!(GroupType::from_wire(-1), GroupType::Normal);
        assert_eq!(GroupType::from_wire(3), GroupType::Normal);
    }
}
