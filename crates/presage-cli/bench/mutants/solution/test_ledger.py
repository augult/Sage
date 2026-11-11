import unittest

from ledger import FEE_FREE_LIMIT, InsufficientFunds, Ledger, LedgerError


class LedgerTest(unittest.TestCase):
    def setUp(self):
        self.ledger = Ledger()
        self.ledger.open("a")
        self.ledger.open("b")
        self.ledger.open("od", overdraft=500)

    # Accounts.

    def test_open_starts_at_zero_with_no_history(self):
        self.assertEqual(self.ledger.balance("a"), 0)
        self.assertEqual(self.ledger.history("a"), [])

    def test_open_rejects_bad_names_and_overdrafts(self):
        for name in ("", None, 7):
            with self.assertRaises(ValueError):
                self.ledger.open(name)
        for overdraft in (-1, 1.5, "10", True):
            with self.assertRaises(ValueError):
                self.ledger.open("x", overdraft=overdraft)

    def test_open_twice(self):
        with self.assertRaises(LedgerError):
            self.ledger.open("a")

    def test_unknown_account_everywhere(self):
        calls = [
            lambda: self.ledger.balance("nope"),
            lambda: self.ledger.deposit("nope", 1),
            lambda: self.ledger.withdraw("nope", 1),
            lambda: self.ledger.transfer("nope", "a", 1),
            lambda: self.ledger.transfer("a", "nope", 1),
            lambda: self.ledger.history("nope"),
            lambda: self.ledger.statement("nope", 1),
            lambda: self.ledger.close("nope"),
        ]
        for call in calls:
            with self.assertRaises(LedgerError):
                call()

    def test_close_needs_a_zero_balance(self):
        self.ledger.deposit("a", 5)
        with self.assertRaises(LedgerError):
            self.ledger.close("a")
        self.ledger.withdraw("od", 5)
        with self.assertRaises(LedgerError):
            self.ledger.close("od")
        self.assertEqual(self.ledger.balance("od"), -5)

    def test_closed_name_is_unknown_and_reopens_empty(self):
        self.ledger.deposit("a", 5)
        self.ledger.withdraw("a", 5)
        self.ledger.close("a")
        with self.assertRaises(LedgerError):
            self.ledger.balance("a")
        self.ledger.open("a")
        self.assertEqual(self.ledger.history("a"), [])
        self.assertEqual(self.ledger.balance("a"), 0)

    # Amounts.

    def test_bad_amounts(self):
        self.ledger.deposit("a", 100)
        for amount in (0, -1, 1.0, "5", None, True, False):
            for call in (
                lambda: self.ledger.deposit("a", amount),
                lambda: self.ledger.withdraw("a", amount),
                lambda: self.ledger.transfer("a", "b", amount),
            ):
                with self.assertRaises(ValueError, msg=repr(amount)):
                    call()
        self.assertEqual(self.ledger.balance("a"), 100)
        self.assertEqual(len(self.ledger.history("a")), 1)

    # Moving money.

    def test_deposit_and_withdraw_return_the_balance(self):
        self.assertEqual(self.ledger.deposit("a", 300), 300)
        self.assertEqual(self.ledger.withdraw("a", 120), 180)
        self.assertEqual(self.ledger.balance("a"), 180)

    def test_withdraw_down_to_the_overdraft_limit(self):
        self.assertEqual(self.ledger.withdraw("od", 500), -500)
        with self.assertRaises(InsufficientFunds):
            self.ledger.withdraw("od", 1)
        self.assertEqual(self.ledger.balance("od"), -500)
        self.assertEqual(len(self.ledger.history("od")), 1)

    def test_withdraw_without_overdraft(self):
        self.ledger.deposit("a", 10)
        self.assertEqual(self.ledger.withdraw("a", 10), 0)
        with self.assertRaises(LedgerError):
            self.ledger.withdraw("a", 1)

    def test_insufficient_funds_is_a_ledger_error(self):
        self.assertTrue(issubclass(InsufficientFunds, LedgerError))

    def test_small_transfer_is_free(self):
        self.ledger.deposit("a", FEE_FREE_LIMIT)
        self.assertEqual(self.ledger.transfer("a", "b", FEE_FREE_LIMIT), 0)
        self.assertEqual(self.ledger.balance("a"), 0)
        self.assertEqual(self.ledger.balance("b"), FEE_FREE_LIMIT)
        self.assertEqual(self.ledger.history("a")[-1], ("transfer-out", FEE_FREE_LIMIT, 0))

    def test_large_transfer_costs_one_percent_rounded_half_up(self):
        self.ledger.deposit("a", 100_000)
        self.assertEqual(self.ledger.transfer("a", "b", 10_001), 100)
        self.assertEqual(self.ledger.transfer("a", "b", 10_050), 101)
        self.assertEqual(self.ledger.transfer("a", "b", 10_049), 100)
        self.assertEqual(self.ledger.balance("a"), 100_000 - 10_001 - 10_050 - 10_049 - 301)
        self.assertEqual(self.ledger.balance("b"), 10_001 + 10_050 + 10_049)
        self.assertEqual(self.ledger.total(), 100_000 - 301)

    def test_transfer_must_cover_the_fee(self):
        self.ledger.deposit("a", 20_100)
        with self.assertRaises(InsufficientFunds):
            self.ledger.transfer("a", "b", 20_000)
        self.assertEqual(self.ledger.balance("a"), 20_100)
        self.assertEqual(self.ledger.balance("b"), 0)
        self.assertEqual(self.ledger.history("b"), [])
        self.ledger.deposit("a", 100)
        self.assertEqual(self.ledger.transfer("a", "b", 20_000), 200)
        self.assertEqual(self.ledger.balance("a"), 0)

    def test_transfer_to_itself(self):
        self.ledger.deposit("a", 50)
        with self.assertRaises(LedgerError):
            self.ledger.transfer("a", "a", 10)
        self.assertEqual(self.ledger.history("a"), [("deposit", 50, 50)])

    def test_failed_transfer_to_unknown_account_changes_nothing(self):
        self.ledger.deposit("a", 50)
        with self.assertRaises(LedgerError):
            self.ledger.transfer("a", "nope", 10)
        self.assertEqual(self.ledger.balance("a"), 50)

    # History.

    def test_history_entries(self):
        self.ledger.deposit("a", 50_000)
        self.ledger.withdraw("a", 1_000)
        self.ledger.transfer("a", "b", 20_000)
        self.assertEqual(
            self.ledger.history("a"),
            [
                ("deposit", 50_000, 50_000),
                ("withdraw", 1_000, 49_000),
                ("transfer-out", 20_000, 29_000),
                ("fee", 200, 28_800),
            ],
        )
        self.assertEqual(self.ledger.history("b"), [("transfer-in", 20_000, 20_000)])

    def test_history_is_a_copy(self):
        self.ledger.deposit("a", 5)
        self.ledger.history("a").clear()
        self.assertEqual(self.ledger.history("a"), [("deposit", 5, 5)])
        self.ledger.statement("a", 1).clear()
        self.assertEqual(self.ledger.statement("a", 1), [("deposit", 5, 5)])

    def test_statement(self):
        for cents in (1, 2, 3):
            self.ledger.deposit("a", cents)
        self.assertEqual(self.ledger.statement("a", 2), [("deposit", 2, 3), ("deposit", 3, 6)])
        self.assertEqual(len(self.ledger.statement("a", 10)), 3)
        self.assertEqual(self.ledger.statement("a", 0), [])
        for last in (-1, 1.0, "2", None):
            with self.assertRaises(ValueError):
                self.ledger.statement("a", last)

    # Interest.

    def test_interest_rounds_half_up_and_skips_non_positive_balances(self):
        self.ledger.deposit("a", 150)
        self.ledger.deposit("b", 149)
        self.ledger.withdraw("od", 400)
        self.ledger.open("tiny")
        self.ledger.deposit("tiny", 49)
        self.assertEqual(self.ledger.apply_interest(100), 3)
        self.assertEqual(self.ledger.balance("a"), 152)
        self.assertEqual(self.ledger.balance("b"), 150)
        self.assertEqual(self.ledger.balance("od"), -400)
        self.assertEqual(self.ledger.history("a")[-1], ("interest", 2, 152))
        self.assertEqual(len(self.ledger.history("od")), 1)
        self.assertEqual(self.ledger.history("tiny"), [("deposit", 49, 49)])

    def test_interest_rate_must_be_a_count(self):
        for rate in (-1, 0.5, "1", True):
            with self.assertRaises(ValueError):
                self.ledger.apply_interest(rate)
        self.assertEqual(self.ledger.apply_interest(0), 0)

    def test_total(self):
        self.ledger.deposit("a", 70)
        self.ledger.withdraw("od", 20)
        self.assertEqual(self.ledger.total(), 50)


if __name__ == "__main__":
    unittest.main()
