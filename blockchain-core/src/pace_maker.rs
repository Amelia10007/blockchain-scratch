use crate::{difficulty::Difficulty, timestamp::Timestamp};
use itertools::Itertools;

pub fn next_difficulty<I>(iter: I, easiest: Difficulty) -> Difficulty
where
    I: IntoIterator<Item = (Difficulty, Timestamp)>,
    I::IntoIter: DoubleEndedIterator + ExactSizeIterator,
{
    let ((diff, stamp3), (_, stamp2), (_, stamp1)) =
        match iter.into_iter().rev().take(3).next_tuple() {
            Some(tuple) => tuple,
            None => return easiest,
        };

    let duration12 = stamp2 - stamp1;
    let duration23 = stamp3 - stamp2;

    if duration12 > duration23 + duration23 {
        diff.raise()
    } else if duration12 + duration12 < duration23 {
        diff.ease().max(easiest)
    } else {
        diff
    }
}
