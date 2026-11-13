//! A summary of one metric's samples.
//!
//! Every measurement in this tool is a handful of samples of the same thing,
//! and one number hides the shape: a median that meets a target while the
//! slowest run is ten times it is not a passing measurement. The summary keeps
//! the whole distribution, so the table in `PLAN.md` can say what it measured
//! and not just that it measured something.

use std::time::Duration;

/// One metric, summarised over its samples.
#[derive(Debug, Clone, Copy)]
pub struct Summary {
    /// How many samples the summary was built from.
    pub samples: usize,
    /// The fastest sample, in milliseconds.
    pub min_ms: f64,
    /// The middle sample, in milliseconds.
    pub median_ms: f64,
    /// The arithmetic mean, in milliseconds.
    pub mean_ms: f64,
    /// The slowest sample, in milliseconds.
    pub max_ms: f64,
}

impl Summary {
    /// Summarises `samples`, or `None` when there is nothing to summarise.
    pub fn of(samples: &[Duration]) -> Option<Self> {
        if samples.is_empty() {
            return None;
        }

        let mut ms: Vec<f64> = samples
            .iter()
            .map(|sample| sample.as_secs_f64() * 1000.0)
            .collect();
        ms.sort_by(|left, right| {
            left.partial_cmp(right)
                .expect("a duration is never NaN, so it always compares")
        });

        let sum: f64 = ms.iter().sum();
        let middle = ms.len() / 2;
        let median = if ms.len().is_multiple_of(2) {
            (ms[middle - 1] + ms[middle]) / 2.0
        } else {
            ms[middle]
        };

        Some(Self {
            samples: ms.len(),
            min_ms: ms[0],
            median_ms: median,
            mean_ms: sum / ms.len() as f64,
            max_ms: ms[ms.len() - 1],
        })
    }

    /// One line, with the median first because that is the number to quote.
    pub fn line(&self, label: &str) -> String {
        format!(
            "{label:<22} median {:>8.2} ms   min {:>7.2}   mean {:>7.2}   max {:>7.2}   n={}",
            self.median_ms, self.min_ms, self.mean_ms, self.max_ms, self.samples
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(values: &[u64]) -> Vec<Duration> {
        values.iter().copied().map(Duration::from_millis).collect()
    }

    #[test]
    fn nothing_to_summarise_is_not_a_summary() {
        assert!(Summary::of(&[]).is_none());
    }

    #[test]
    fn the_median_is_the_middle_sample() {
        let summary = Summary::of(&ms(&[30, 10, 20])).expect("three samples");
        assert_eq!(summary.median_ms, 20.0);
        assert_eq!(summary.min_ms, 10.0);
        assert_eq!(summary.max_ms, 30.0);
        assert_eq!(summary.samples, 3);
    }

    #[test]
    fn an_even_count_averages_the_two_middle_samples() {
        let summary = Summary::of(&ms(&[40, 10, 30, 20])).expect("four samples");
        assert_eq!(summary.median_ms, 25.0);
        assert_eq!(summary.mean_ms, 25.0);
    }

    #[test]
    fn one_sample_is_its_own_everything() {
        let summary = Summary::of(&ms(&[7])).expect("one sample");
        assert_eq!(summary.median_ms, 7.0);
        assert_eq!(summary.min_ms, 7.0);
        assert_eq!(summary.max_ms, 7.0);
    }
}
