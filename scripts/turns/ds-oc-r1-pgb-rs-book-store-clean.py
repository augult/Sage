THOUGHT = """
The API is `pub fn lowest_price(books: &[u32]) -> u32`, returning cents.

Plan: count copies of each of the 5 titles, then greedily peel off the largest possible group of distinct titles (5, then 4, ...). The only case where that greedy is suboptimal is a group of 5 plus a group of 3, which is cheaper as two groups of 4 (3000 + 2160 = 5160 vs 2*2560 = 5120), so swap those pairs afterwards. Price each group from a cents table.
"""

CONTENT = """
The stub is `lowest_price(books: &[u32]) -> u32` returning cents. I'll count copies per title, greedily form the largest distinct-title groups, then apply the 5+3 -> 4+4 correction, which is the only case where greedy is suboptimal.
"""

CALLS = [
    {"name": "write", "arguments": {"filePath": "src/lib.rs", "content": """/// Price in cents of a single book.
const BOOK_PRICE: u32 = 800;

/// Price in cents of a group of `n` distinct books.
fn group_price(n: usize) -> u32 {
    let discount_percent = match n {
        0 => 0,
        1 => 0,
        2 => 5,
        3 => 10,
        4 => 20,
        _ => 25,
    };
    (n as u32) * BOOK_PRICE * (100 - discount_percent) / 100
}

pub fn lowest_price(books: &[u32]) -> u32 {
    let mut counts = [0usize; 5];
    for &book in books {
        if (1..=5).contains(&book) {
            counts[(book - 1) as usize] += 1;
        }
    }

    // groups[n] = how many groups of n distinct books we formed.
    let mut groups = [0usize; 6];
    loop {
        counts.sort_unstable_by(|a, b| b.cmp(a));
        let distinct = counts.iter().filter(|&&c| c > 0).count();
        if distinct == 0 {
            break;
        }
        for count in counts.iter_mut().take(distinct) {
            *count -= 1;
        }
        groups[distinct] += 1;
    }

    // A group of 5 plus a group of 3 costs 5160, but two groups of 4 cost 5120.
    let swaps = groups[5].min(groups[3]);
    groups[5] -= swaps;
    groups[3] -= swaps;
    groups[4] += 2 * swaps;

    (1..=5).map(|n| groups[n] as u32 * group_price(n)).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_book_costs_full_price() {
        assert_eq!(lowest_price(&[1]), 800);
    }

    #[test]
    fn two_different_books_get_five_percent_off() {
        assert_eq!(lowest_price(&[1, 2]), 1520);
    }

    #[test]
    fn three_different_books_get_ten_percent_off() {
        assert_eq!(lowest_price(&[1, 2, 3]), 2160);
    }

    #[test]
    fn four_different_books_get_twenty_percent_off() {
        assert_eq!(lowest_price(&[1, 2, 3, 4]), 2560);
    }

    #[test]
    fn five_different_books_get_twenty_five_percent_off() {
        assert_eq!(lowest_price(&[1, 2, 3, 4, 5]), 3000);
    }

    #[test]
    fn two_copies_of_each_book() {
        assert_eq!(lowest_price(&[1, 1, 2, 2, 3, 3, 4, 4, 5, 5]), 6000);
    }

    #[test]
    fn readme_example_prefers_two_groups_of_four() {
        assert_eq!(lowest_price(&[1, 1, 2, 2, 3, 3, 4, 5]), 5120);
    }

    #[test]
    fn empty_basket_is_free() {
        assert_eq!(lowest_price(&[]), 0);
    }
}
"""}},
    {"name": "bash", "arguments": {"command": "cargo test 2>&1 | tail -40"}},
]
