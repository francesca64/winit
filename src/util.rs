use std::ops::BitAnd;

// Replace with `!` once stable
#[derive(Debug)]
pub enum Never {}

pub fn has_flag<T>(bitset: T, flag: T) -> bool
where T:
    Copy + PartialEq + BitAnd<T, Output = T>
{
    bitset & flag == flag
}
