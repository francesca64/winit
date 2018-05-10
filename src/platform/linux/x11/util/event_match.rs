use std::os::raw::c_int;

// Super sophisticated trait for event matching awesomification
pub trait EventMatch<'a, T> {
    fn get_match(&'a self, base_code: c_int) -> T;
}
