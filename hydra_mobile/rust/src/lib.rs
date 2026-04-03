pub mod api;
mod credit_runtime;
mod frb_generated;
mod ios_extension;
mod provider_runtime;

#[cfg(test)]
pub mod test_support {
    use lazy_static::lazy_static;
    use std::sync::Mutex;

    lazy_static! {
        pub static ref GLOBAL_TEST_GUARD: Mutex<()> = Mutex::new(());
    }
}
