#[macro_export]
macro_rules! result {
    ($wrapper:ident, $inner:ty) => {
        #[derive(Default, Clone)]
        pub struct $wrapper {
            inner: Option<$inner>,
            error: Option<String>,
        }

        impl $wrapper {
            pub fn new() -> Self {
                Self {
                    inner: None,
                    error: None,
                }
            }

            pub fn set(&mut self, value: $inner) {
                self.inner = Some(value);
                self.error = None;
            }

            pub fn set_error(&mut self, error: String) {
                self.error = Some(error);
                self.inner = None;
            }

            pub fn unwrap(&self) -> $inner {
                self.inner.clone().unwrap()
            }

            pub fn boxed(&self) -> Box<$inner> {
                Box::new(self.inner.clone().unwrap())
            }

            pub fn error(&self) -> String {
                self.error.clone().unwrap_or_default()
            }

            pub fn is_ok(&self) -> bool {
                self.inner.is_some() && self.error.is_none()
            }

            pub fn is_err(&self) -> bool {
                self.inner.is_none() && self.error.is_some()
            }
        }
    };
}

#[macro_export]
macro_rules! declare {
    ($wrapper:ident) => {
        extern "Rust" {
            type $wrapper;
            fn is_ok(&self) -> bool;
            fn is_err(&self) -> bool;
            fn error(&self) -> String;
            fn mnemonic_from_string(value: String) -> Box<$wrapper>;
        }
    };
}
