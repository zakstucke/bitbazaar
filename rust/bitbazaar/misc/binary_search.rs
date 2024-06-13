use std::cmp::Ordering;

/// Binary search for the desired item in the array.
/// Returns None when not exactly found.
///
/// Arguments:
/// - arr: The array to search.
/// - comparer: Fn(&Item) that should return `$target.cmp($item_val)`. (i.e. "where to go from here")
///
/// Returns:
/// - Some(index) the index of the item in the array.
/// - None if the item is not found.
pub fn binary_search_exact<Item>(
    arr: &[Item],
    comparer: impl Fn(&Item) -> Ordering,
) -> Option<usize> {
    let mut low = 0;
    let mut high = arr.len() - 1;
    while low <= high {
        let mid = (low + high) / 2;
        match comparer(&arr[mid]) {
            Ordering::Greater => low = mid + 1,
            Ordering::Less => high = mid - 1,
            Ordering::Equal => return Some(mid),
        }
    }
    None
}

/// Binary search for the desired item in the array.
/// Only returns None when the array is empty.
///
/// Arguments:
/// - arr: The array to search.
/// - comparer: Fn(&Item) that should return `$target.cmp($item_val)`. (i.e. "where to go from here")
///
/// Returns:
/// - Some(index) the index of the item in the array, OR THE CLOSEST.
/// - None if the item is not found.
pub fn binary_search_soft<Item>(
    arr: &[Item],
    comparer: impl Fn(&Item) -> Ordering,
) -> Option<usize> {
    if arr.is_empty() {
        None
    } else {
        let mut low = 0;
        let mut high = arr.len() - 1;
        let mut mid = 0;
        while low <= high {
            mid = (low + high) / 2;
            match comparer(&arr[mid]) {
                Ordering::Greater => low = mid + 1,
                Ordering::Less => high = mid - 1,
                Ordering::Equal => return Some(mid),
            }
        }
        Some(mid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binary_search() {
        // 7 is missing, with exact should be None, with soft should be 5
        let arr = [1, 2, 3, 4, 5, 6, 8, 9, 10];
        assert_eq!(binary_search_exact(&arr, |x| 5.cmp(x)), Some(4));
        assert_eq!(binary_search_exact(&arr, |x| 1.cmp(x)), Some(0));
        assert_eq!(binary_search_exact(&arr, |x| 10.cmp(x)), Some(8));
        assert_eq!(binary_search_exact(&arr, |x| 7.cmp(x)), None);

        assert_eq!(binary_search_soft(&arr, |x| 5.cmp(x)), Some(4));
        assert_eq!(binary_search_soft(&arr, |x| 1.cmp(x)), Some(0));
        assert_eq!(binary_search_soft(&arr, |x| 10.cmp(x)), Some(8));
        assert_eq!(binary_search_soft(&arr, |x| 7.cmp(x)), Some(5));
        assert_eq!(binary_search_soft(&[], |x| 7.cmp(x)), None);
    }
}
