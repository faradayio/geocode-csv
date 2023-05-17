/// Tools for measuring the size of a data structure.
use std::{mem::size_of, ops::Deref};

use bigtable_rs::bigtable::RowCell;

/// Measure the memory size of a data structure.
pub trait MemoryUsed: Sized {
    /// Measure the memory size of a data structure, including itself.
    fn memory_used(&self) -> usize {
        self.memory_owned() + size_of::<Self>()
    }

    /// Measured the memory size of everything owned by a data structure, but
    /// not the structure itself.
    fn memory_owned(&self) -> usize {
        0
    }
}

impl<T> MemoryUsed for Box<T>
where
    T: MemoryUsed,
{
    fn memory_owned(&self) -> usize {
        // Get a reference to our boxed value, stripped of the `Box` type.
        let boxed_value: &T = self.deref();
        // `memory_used` is correct here, because it includes the store for one
        // `T` value, plus anything owned by that `T`.
        boxed_value.memory_used()
    }
}

impl<T> MemoryUsed for Vec<T>
where
    T: MemoryUsed,
{
    fn memory_owned(&self) -> usize {
        let item_mem = self.iter().map(|item| item.memory_owned()).sum::<usize>();
        let capacity_mem = self.capacity() * size_of::<T>();
        item_mem + capacity_mem
    }
}

impl<T, U> MemoryUsed for (T, U)
where
    T: MemoryUsed,
    U: MemoryUsed,
{
    fn memory_owned(&self) -> usize {
        self.0.memory_owned() + self.1.memory_owned()
    }
}

impl MemoryUsed for u8 {}

impl MemoryUsed for i64 {}

impl MemoryUsed for String {
    fn memory_owned(&self) -> usize {
        self.capacity()
    }
}

impl MemoryUsed for RowCell {
    fn memory_owned(&self) -> usize {
        self.family_name.memory_owned()
            + self.qualifier.memory_owned()
            + self.value.memory_owned()
            + self.timestamp_micros.memory_owned()
            + self.labels.memory_owned()
    }
}
