pub mod api;
mod frb_generated;

#[cfg(test)]
pub mod test_support {
    use lazy_static::lazy_static;
    use std::sync::Mutex;

    lazy_static! {
        pub static ref GLOBAL_TEST_GUARD: Mutex<()> = Mutex::new(());
    }
}
