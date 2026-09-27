//! Discrete-event queue. Events fire in time order; ties break by insertion order, so
//! processing is deterministic.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

struct Entry<E> {
    time: f64,
    seq: u64,
    event: E,
}

impl<E> PartialEq for Entry<E> {
    fn eq(&self, o: &Self) -> bool {
        self.cmp(o) == Ordering::Equal
    }
}
impl<E> Eq for Entry<E> {}
impl<E> PartialOrd for Entry<E> {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl<E> Ord for Entry<E> {
    /// Reversed so the max-heap pops the earliest event first.
    fn cmp(&self, o: &Self) -> Ordering {
        o.time.total_cmp(&self.time).then(o.seq.cmp(&self.seq))
    }
}

pub struct Scheduler<E> {
    heap: BinaryHeap<Entry<E>>,
    seq: u64,
}

impl<E> Default for Scheduler<E> {
    fn default() -> Self {
        Self { heap: BinaryHeap::new(), seq: 0 }
    }
}

impl<E> Scheduler<E> {
    pub fn schedule(&mut self, time: f64, event: E) {
        self.heap.push(Entry { time, seq: self.seq, event });
        self.seq += 1;
    }

    /// Remove and return the earliest event if it is due at or before `t`.
    pub fn pop_due(&mut self, t: f64) -> Option<(f64, E)> {
        if self.heap.peek()?.time <= t {
            self.heap.pop().map(|e| (e.time, e.event))
        } else {
            None
        }
    }

    pub fn next_time(&self) -> Option<f64> {
        self.heap.peek().map(|e| e.time)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fires_in_time_order_with_stable_ties() {
        let mut s = Scheduler::default();
        s.schedule(5.0, "c");
        s.schedule(1.0, "a");
        s.schedule(5.0, "d");
        s.schedule(2.0, "b");
        let mut out = vec![];
        while let Some((_, e)) = s.pop_due(10.0) {
            out.push(e);
        }
        assert_eq!(out, ["a", "b", "c", "d"]);
    }

    #[test]
    fn does_not_fire_future_events() {
        let mut s = Scheduler::default();
        s.schedule(3.0, ());
        assert!(s.pop_due(2.9).is_none());
        assert!(s.pop_due(3.0).is_some());
    }
}
