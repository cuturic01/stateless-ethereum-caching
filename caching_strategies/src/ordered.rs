use std::hash::Hash;

use crate::hash::FastMap;

const NONE: usize = usize::MAX;

struct Node<K, V> {
    key: K,
    val: V,
    prev: usize,
    next: usize,
}

pub struct OrderedMap<K, V> {
    nodes: Vec<Node<K, V>>,
    free: Vec<usize>,
    index: FastMap<K, usize>,
    head: usize, // front / LRU
    tail: usize, // back / MRU
}

impl<K, V> Default for OrderedMap<K, V> {
    fn default() -> Self {
        OrderedMap {
            nodes: Vec::new(),
            free: Vec::new(),
            index: FastMap::default(),
            head: NONE,
            tail: NONE,
        }
    }
}

impl<K: Copy + Eq + Hash, V> OrderedMap<K, V> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    pub fn contains(&self, key: &K) -> bool {
        self.index.contains_key(key)
    }

    fn alloc(&mut self, key: K, val: V) -> usize {
        if let Some(slot) = self.free.pop() {
            self.nodes[slot] = Node { key, val, prev: NONE, next: NONE };
            slot
        } else {
            self.nodes.push(Node { key, val, prev: NONE, next: NONE });
            self.nodes.len() - 1
        }
    }

    fn unlink(&mut self, i: usize) {
        let (p, n) = (self.nodes[i].prev, self.nodes[i].next);
        if p != NONE {
            self.nodes[p].next = n;
        } else {
            self.head = n;
        }
        if n != NONE {
            self.nodes[n].prev = p;
        } else {
            self.tail = p;
        }
        self.nodes[i].prev = NONE;
        self.nodes[i].next = NONE;
    }

    fn link_back(&mut self, i: usize) {
        let t = self.tail;
        self.nodes[i].prev = t;
        self.nodes[i].next = NONE;
        if t != NONE {
            self.nodes[t].next = i;
        } else {
            self.head = i;
        }
        self.tail = i;
    }

    pub fn push_back(&mut self, key: K, val: V) {
        debug_assert!(!self.index.contains_key(&key));
        let i = self.alloc(key, val);
        self.index.insert(key, i);
        self.link_back(i);
    }

    pub fn touch_back(&mut self, key: &K, val: V) -> bool {
        if let Some(&i) = self.index.get(key) {
            self.unlink(i);
            self.nodes[i].val = val;
            self.link_back(i);
            true
        } else {
            false
        }
    }

    pub fn remove(&mut self, key: &K) -> Option<V>
    where
        V: Default,
    {
        if let Some(i) = self.index.remove(key) {
            self.unlink(i);
            let val = std::mem::take(&mut self.nodes[i].val);
            self.free.push(i);
            Some(val)
        } else {
            None
        }
    }

    pub fn front(&self) -> Option<(K, &V)> {
        if self.head == NONE {
            None
        } else {
            let n = &self.nodes[self.head];
            Some((n.key, &n.val))
        }
    }

    pub fn pop_front(&mut self) -> Option<(K, V)>
    where
        V: Default,
    {
        if self.head == NONE {
            return None;
        }
        let i = self.head;
        let key = self.nodes[i].key;
        self.index.remove(&key);
        self.unlink(i);
        let val = std::mem::take(&mut self.nodes[i].val);
        self.free.push(i);
        Some((key, val))
    }
}
