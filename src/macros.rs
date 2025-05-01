#[macro_export]
macro_rules! results {
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
macro_rules! result {
    ($struct_name:ident, $inner:ty) => {
        #[derive(Debug, Clone)]
        pub struct $struct_name {
            value: Option<$inner>,
            error: Option<String>,
        }
        impl $struct_name {
            #[allow(clippy::new_without_default)]
            pub fn new() -> Self {
                Self {
                    value: None,
                    error: None,
                }
            }
            pub fn ok(value: $inner) -> Self {
                Self {
                    value: Some(value),
                    error: None,
                }
            }
            pub fn err(error: &str) -> Self {
                Self {
                    value: None,
                    error: Some(error.into()),
                }
            }
            pub fn is_ok(&self) -> bool {
                self.value.is_some() && self.error.is_none()
            }
            pub fn is_err(&self) -> bool {
                self.value.is_none() && self.error.is_some()
            }
            pub fn value(&self) -> $inner {
                self.value.clone().unwrap()
            }
            pub fn error(&self) -> String {
                self.error.clone().unwrap()
            }
            pub fn boxed(&self) -> Box<Self> {
                Box::new(self.clone())
            }
        }
        impl From<&str> for Box<$struct_name> {
            fn from(value: &str) -> Box<$struct_name> {
                Box::new($struct_name {
                    value: None,
                    error: Some(value.into()),
                })
            }
        }
    };
}
