//! Pure date-preview state; window effects are applied by App and never saved while previewing.
use chrono::NaiveDate;
use std::collections::HashMap;
use std::hash::Hash;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Visibility {
    pub shown: bool,
    pub minimized: bool,
}

pub struct DatePreview<K> {
    pub previous: Option<NaiveDate>,
    pub original: HashMap<K, Visibility>,
}

impl<K: Copy + Eq + Hash> DatePreview<K> {
    /// Each choice replaces the preview, never its original rollback snapshot or scope.
    pub fn choose(&self, day: Option<NaiveDate>, dates: &HashMap<K, NaiveDate>) -> HashMap<K, bool> {
        self.original.keys().map(|id| (*id, day.is_none() || dates.get(id).copied() == day)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_choices_empty_days_and_rollback_keep_original_visibility() {
        let a = NaiveDate::from_ymd_opt(2026, 11, 13).unwrap();
        let b = a.pred_opt().unwrap();
        let original = HashMap::from([
            (1, Visibility { shown: true, minimized: true }),
            (2, Visibility { shown: false, minimized: false }),
        ]);
        let preview = DatePreview { previous: Some(a), original: original.clone() };
        // An out-of-scope note never enters a preview, even if its date matches.
        let dates = HashMap::from([(1, a), (2, b), (3, a)]);
        assert_eq!(preview.choose(Some(a), &dates), HashMap::from([(1, true), (2, false)]));
        assert_eq!(preview.choose(Some(b), &dates), HashMap::from([(1, false), (2, true)]));
        assert!(preview.choose(Some(a.succ_opt().unwrap()), &dates).values().all(|on| !on));
        assert!(preview.choose(None, &dates).values().all(|on| *on));
        assert_eq!(preview.original, original);
        assert_eq!(preview.previous, Some(a));
    }
}
