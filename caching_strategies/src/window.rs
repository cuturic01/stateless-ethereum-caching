use crate::model::Key;
use crate::ordered::OrderedMap;

pub struct RetentionWindow {
    n: u64,
    order: OrderedMap<u64>,
}

impl RetentionWindow {
    pub fn new(n: u64) -> Self {
        RetentionWindow { n, order: OrderedMap::new() }
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    pub fn touch(&mut self, key: Key, now: u64) {
        if !self.order.touch_back(&key, now) {
            self.order.push_back(key, now);
        }
    }

    pub fn remove(&mut self, key: &Key) -> Option<u64> {
        self.order.remove(key)
    }

    pub fn drain_expired(&mut self, now: u64) -> Vec<(Key, u64)> {
        let mut expired = Vec::new();
        while let Some((_, &last_seen)) = self.order.front() {
            if last_seen + self.n <= now {
                let (k, v) = self.order.pop_front().expect("front existed");
                expired.push((k, v));
            } else {
                break;
            }
        }
        expired
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Address, ADDR_LEN};

    fn k(n: u8) -> Key {
        Key::account(Address([n; ADDR_LEN]))
    }

    #[test]
    fn expires_after_n_blocks() {
        let mut w = RetentionWindow::new(4);
        w.touch(k(1), 100);
        assert!(w.drain_expired(103).is_empty());
        let exp = w.drain_expired(104);
        assert_eq!(exp, vec![(k(1), 100)]);
        assert!(w.is_empty());
    }

    #[test]
    fn refresh_extends_lifetime() {
        let mut w = RetentionWindow::new(4);
        w.touch(k(1), 100);
        w.touch(k(1), 102); // refreshed
        assert!(w.drain_expired(104).is_empty()); // 102 + 4 = 106 > 104
        let exp = w.drain_expired(106);
        assert_eq!(exp, vec![(k(1), 102)]);
    }
}
