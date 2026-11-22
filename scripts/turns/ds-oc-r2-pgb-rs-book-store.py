THOUGHT = r'''Five distinct titles, prices in cents, maximize discount.

Count copies per title, then peel off maximal sets greedily (each set takes one copy of every title still in stock). That is optimal except that a 5-set plus a 3-set (35% total) is worse than two 4-sets (40%), so swap each available (5,3) pair into (4,4).

Prices: 1 = 800, 2 = 1520, 3 = 2160, 4 = 2560, 5 = 3000 cents.'''
CONTENT = r'''Implementing the grouping algorithm and verifying with cargo test.'''
SRC = r'''pub fn lowest_price(books: &[u32]) -> u32 {
    let mut counts = [0u32; 5];
    for &book in books {
        if (1..=5).contains(&book) {
            counts[(book - 1) as usize] += 1;
        }
    }

    // Peel off sets: each set takes one copy of every title still in stock.
    let mut groups = [0u32; 6];
    loop {
        let distinct = counts.iter().filter(|&&c| c > 0).count();
        if distinct == 0 {
            break;
        }
        for count in counts.iter_mut() {
            if *count > 0 {
                *count -= 1;
            }
        }
        groups[distinct] += 1;
    }

    // A set of 5 plus a set of 3 is a worse discount than two sets of 4.
    let swaps = groups[5].min(groups[3]);
    groups[5] -= swaps;
    groups[3] -= swaps;
    groups[4] += 2 * swaps;

    let discount = [0u32, 0, 5, 10, 20, 25];
    (1..=5)
        .map(|size| groups[size] * size * 800 * (100 - discount[size]) / 100)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_basket() {
        assert_eq!(lowest_price(&[]), 0);
    }

    #[test]
    fn simple_discounts() {
        assert_eq!(lowest_price(&[1]), 800);
        assert_eq!(lowest_price(&[1, 2]), 1520);
        assert_eq!(lowest_price(&[1, 2, 3]), 2160);
        assert_eq!(lowest_price(&[1, 2, 3, 4]), 2560);
        assert_eq!(lowest_price(&[1, 2, 3, 4, 5]), 3000);
    }

    #[test]
    fn prefers_two_fours_over_five_plus_three() {
        assert_eq!(lowest_price(&[1, 1, 2, 2, 3, 3, 4, 5]), 5120);
    }
}
'''
CALLS = [
    {"name": "write", "arguments": {"filePath": "src/lib.rs", "content": SRC}},
    {"name": "bash", "arguments": {"command": "cargo test 2>&1 | tail -20", "timeout": 120}},
]
