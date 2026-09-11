//! Explicit current source contributors for canonical establishment geometry.

use std::fmt;

use uuid::Uuid;

const MAX_CONFIRMED_SOURCES: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfirmedSourceSet {
    source_ids: Vec<Uuid>,
}

impl ConfirmedSourceSet {
    pub fn new(mut source_ids: Vec<Uuid>) -> Result<Self, InvalidConfirmedSourceSet> {
        if source_ids.is_empty() {
            return Err(InvalidConfirmedSourceSet::Empty);
        }
        if source_ids.len() > MAX_CONFIRMED_SOURCES {
            return Err(InvalidConfirmedSourceSet::TooMany);
        }
        source_ids.sort_unstable();
        if source_ids.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(InvalidConfirmedSourceSet::Duplicate);
        }
        Ok(Self { source_ids })
    }

    pub fn source_ids(&self) -> &[Uuid] {
        &self.source_ids
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvalidConfirmedSourceSet {
    Empty,
    TooMany,
    Duplicate,
}

impl fmt::Display for InvalidConfirmedSourceSet {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Empty => "confirmed source set must not be empty",
            Self::TooMany => "confirmed source set exceeds its bounded size",
            Self::Duplicate => "confirmed source set contains a duplicate source",
        })
    }
}

impl std::error::Error for InvalidConfirmedSourceSet {}

#[cfg(test)]
mod tests {
    use super::{ConfirmedSourceSet, InvalidConfirmedSourceSet};
    use uuid::Uuid;

    #[test]
    fn contributor_set_is_nonempty_bounded_unique_and_order_independent() {
        let first = Uuid::from_u128(1);
        let second = Uuid::from_u128(2);

        assert_eq!(
            ConfirmedSourceSet::new(vec![]),
            Err(InvalidConfirmedSourceSet::Empty)
        );
        assert_eq!(
            ConfirmedSourceSet::new(vec![first, first]),
            Err(InvalidConfirmedSourceSet::Duplicate)
        );
        assert_eq!(
            ConfirmedSourceSet::new(vec![second, first])
                .expect("distinct contributors must be accepted")
                .source_ids(),
            &[first, second]
        );
    }
}
