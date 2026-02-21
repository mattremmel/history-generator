/// Monotonic ID generator shared across all entity types.
/// Guarantees globally unique IDs — no two objects of any type share an ID.
#[derive(Debug)]
pub struct IdGenerator {
    next: u64,
}

impl IdGenerator {
    pub fn new() -> Self {
        Self { next: 1 }
    }

    pub fn starting_from(start: u64) -> Self {
        Self { next: start }
    }

    pub fn next_id(&mut self) -> u64 {
        let id = self.next;
        self.next += 1;
        id
    }
}

impl Default for IdGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequential_ids() {
        let mut id_gen = IdGenerator::new();
        assert_eq!(id_gen.next_id(), 1);
        assert_eq!(id_gen.next_id(), 2);
        assert_eq!(id_gen.next_id(), 3);
    }

    #[test]
    fn starting_from() {
        let mut id_gen = IdGenerator::starting_from(100);
        assert_eq!(id_gen.next_id(), 100);
        assert_eq!(id_gen.next_id(), 101);
    }
}
