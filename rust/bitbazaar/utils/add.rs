pub fn add(a: f64, b: f64) -> f64 {
    a + b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add() {
        let result = add(1.0, 2.0);
        assert_eq!(result, 3.0);
    }
}
