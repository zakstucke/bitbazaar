use crate::prelude::*;

macro_rules! try_fallbacks {
    ($e1:expr, $value:expr) => {
        match Self::from_last(&$value) {
            Ok(deserialized) => Ok(deserialized),
            Err(e2) => match Self::from_next(&$value) {
                Ok(deserialized) => Ok(deserialized),
                Err(e3) => Err(anyerr!(
                    "Failed to deserialize to target directly or through either migratable type."
                )
                .attach_printable(format!("direct error: {:?}", $e1))
                .attach_printable(format!("from_last error: {:?}", e2))
                .attach_printable(format!("from_next error: {:?}", e3))),
            },
        }
    };
}

/// A trait to help with migrating data structures when they change.
pub trait SerdeMigratable: Sized + serde::de::DeserializeOwned {
    /// How to convert from the last version to the current version.
    fn from_last(last: &serde_json::Value) -> RResult<Self, AnyErr>;

    /// Optional, how to convert from the next back to this, can be useful if rolling back.
    fn from_next(_next: &serde_json::Value) -> RResult<Self, AnyErr> {
        Err(anyerr!(
            "Not implemented! Need to implement 'from_next' method on the 'SerdeMigratable' trait for rollback to work."
        ))
    }

    /// Deserialize from a string, trying to convert from legacy types if needed.
    fn from_str(src: &str) -> RResult<Self, AnyErr> {
        match serde_json::from_str::<Self>(src).change_context(AnyErr) {
            Ok(deserialized) => Ok(deserialized),
            Err(e1) => {
                let value: serde_json::Value = serde_json::from_str(src).change_context(AnyErr)?;
                try_fallbacks!(e1, value)
            }
        }
    }

    /// Deserialize from a slice, trying to convert from legacy types if needed.
    fn from_slice(src: &[u8]) -> RResult<Self, AnyErr> {
        match serde_json::from_slice::<Self>(src).change_context(AnyErr) {
            Ok(deserialized) => Ok(deserialized),
            Err(e1) => {
                let value: serde_json::Value =
                    serde_json::from_slice(src).change_context(AnyErr)?;
                try_fallbacks!(e1, value)
            }
        }
    }

    /// Deserialize from a value, trying to convert from legacy types if needed.
    fn from_value(src: serde_json::Value) -> RResult<Self, AnyErr> {
        match serde_json::from_value::<Self>(src.clone()).change_context(AnyErr) {
            Ok(deserialized) => Ok(deserialized),
            Err(e1) => try_fallbacks!(e1, src),
        }
    }

    /// Deserialize from a reader, trying to convert from legacy types if needed.
    fn from_reader<R: std::io::Read>(mut src: R) -> RResult<Self, AnyErr> {
        let mut buffer = Vec::new();
        src.read_to_end(&mut buffer).change_context(AnyErr)?;
        match serde_json::from_slice::<Self>(&buffer).change_context(AnyErr) {
            Ok(deserialized) => Ok(deserialized),
            Err(e1) => {
                let value: serde_json::Value =
                    serde_json::from_slice(&buffer).change_context(AnyErr)?;
                try_fallbacks!(e1, value)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    use super::*;
    use crate::test::prelude::*;

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct V1 {
        v1: String,
    }

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct V2 {
        v2: String,
    }

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct V3 {
        v3: String,
    }

    impl SerdeMigratable for V2 {
        fn from_last(last: &serde_json::Value) -> RResult<Self, AnyErr> {
            let v1: V1 = serde_json::from_value(last.clone()).change_context(AnyErr)?;
            Ok(Self { v2: v1.v1 })
        }

        fn from_next(next: &serde_json::Value) -> RResult<Self, AnyErr> {
            let v3: V3 = serde_json::from_value(next.clone()).change_context(AnyErr)?;
            Ok(Self { v2: v3.v3 })
        }
    }

    fn setup() -> RResult<(V2, String, String, String), AnyErr> {
        let v1 = V1 {
            v1: "hello".to_string(),
        };
        let v1_str = serde_json::to_string(&v1).change_context(AnyErr)?;
        let v2 = V2 {
            v2: "hello".to_string(),
        };
        let v2_str = serde_json::to_string(&v2).change_context(AnyErr)?;
        let v3 = V3 {
            v3: "hello".to_string(),
        };
        let v3_str = serde_json::to_string(&v3).change_context(AnyErr)?;
        Ok((v2, v1_str, v2_str, v3_str))
    }

    #[rstest]
    fn test_serde_migratable_from_str() -> RResult<(), AnyErr> {
        let (v2, v1_str, v2_str, v3_str) = setup()?;
        assert_eq!(v2, V2::from_str(&v2_str)?);
        assert_eq!(v2, V2::from_str(&v1_str)?);
        assert_eq!(v2, V2::from_str(&v3_str)?);
        Ok(())
    }

    #[rstest]
    fn test_serde_migratable_from_slice() -> RResult<(), AnyErr> {
        let (v2, v1_str, v2_str, v3_str) = setup()?;
        assert_eq!(v2, V2::from_slice(v2_str.as_bytes())?);
        assert_eq!(v2, V2::from_slice(v1_str.as_bytes())?);
        assert_eq!(v2, V2::from_slice(v3_str.as_bytes())?);
        Ok(())
    }

    #[rstest]
    fn test_serde_migratable_from_value() -> RResult<(), AnyErr> {
        let (v2, v1_str, v2_str, v3_str) = setup()?;
        assert_eq!(
            v2,
            V2::from_value(serde_json::from_str(&v2_str).change_context(AnyErr)?)?
        );
        assert_eq!(
            v2,
            V2::from_value(serde_json::from_str(&v1_str).change_context(AnyErr)?)?
        );
        assert_eq!(
            v2,
            V2::from_value(serde_json::from_str(&v3_str).change_context(AnyErr)?)?
        );
        Ok(())
    }

    #[rstest]
    fn test_serde_migratable_from_reader() -> RResult<(), AnyErr> {
        let (v2, v1_str, v2_str, v3_str) = setup()?;
        assert_eq!(
            v2,
            V2::from_reader(std::io::Cursor::new(v2_str.as_bytes()))?
        );
        assert_eq!(
            v2,
            V2::from_reader(std::io::Cursor::new(v1_str.as_bytes()))?
        );
        assert_eq!(
            v2,
            V2::from_reader(std::io::Cursor::new(v3_str.as_bytes()))?
        );
        Ok(())
    }
}
