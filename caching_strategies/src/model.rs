use std::fmt;

pub const ADDR_LEN: usize = 20;
pub const SLOT_LEN: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Address(pub [u8; ADDR_LEN]);

impl Address {
    pub fn to_hex(self) -> String {
        let mut s = String::with_capacity(2 + ADDR_LEN * 2);
        s.push_str("0x");
        for b in self.0 {
            s.push_str(&format!("{b:02x}"));
        }
        s
    }

    pub fn from_hex(s: &str) -> Result<Self, String> {
        let h = s.strip_prefix("0x").unwrap_or(s);
        if h.len() != ADDR_LEN * 2 {
            return Err(format!("address hex must be {} chars, got {}", ADDR_LEN * 2, h.len()));
        }
        let mut out = [0u8; ADDR_LEN];
        for (i, byte) in out.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&h[i * 2..i * 2 + 2], 16)
                .map_err(|e| format!("bad hex in address: {e}"))?;
        }
        Ok(Address(out))
    }
}

impl fmt::Debug for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Address({})", self.to_hex())
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Key {
    pub addr: Address,
    pub slot: Option<[u8; SLOT_LEN]>,
}

impl Key {
    pub fn account(addr: Address) -> Self {
        Key { addr, slot: None }
    }
    pub fn storage(addr: Address, slot: [u8; SLOT_LEN]) -> Self {
        Key { addr, slot: Some(slot) }
    }
}

impl fmt::Debug for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.slot {
            None => write!(f, "Key({}, header)", self.addr.to_hex()),
            Some(s) => {
                write!(f, "Key({}, slot 0x", self.addr.to_hex())?;
                for b in &s[..4] {
                    write!(f, "{b:02x}")?;
                }
                write!(f, "..)")
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct BlockRecord {
    pub block_number: u64,
    pub timestamp: u64,
    pub reads: Vec<Key>,
    pub writes: Vec<Key>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_hex_roundtrip() {
        let a = Address([0xab; ADDR_LEN]);
        let h = a.to_hex();
        assert_eq!(h.len(), 42);
        assert_eq!(Address::from_hex(&h).unwrap(), a);
        // matches a known lower-case strata address
        let usdt = "0xdac17f958d2ee523a2206206994597c13d831ec7";
        assert_eq!(Address::from_hex(usdt).unwrap().to_hex(), usdt);
    }

    #[test]
    fn key_variants_distinct() {
        let a = Address([1; ADDR_LEN]);
        assert_ne!(Key::account(a), Key::storage(a, [0; SLOT_LEN]));
    }
}
