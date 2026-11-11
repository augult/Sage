import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from fetchkit import Client, RetryError
from fetchkit.transport import SimulatedTransport

URL = "https://mirror.example/packages/core.tar"


def client(fail_first, **settings):
    return Client(transport=SimulatedTransport(fail_first=fail_first, trace=""), sleep=lambda _: None, **settings)


class ClientTest(unittest.TestCase):
    def test_fetches(self):
        self.assertEqual(client(0).fetch(URL), f"content of {URL}\n".encode())

    def test_two_attempts(self):
        made = client(1, attempts=2)
        made.fetch(URL)
        self.assertEqual(made.transport.calls[URL], 2)

    def test_two_attempts_are_not_three(self):
        made = client(2, attempts=2)
        with self.assertRaises(RetryError):
            made.fetch(URL)
        self.assertEqual(made.transport.calls[URL], 2)


if __name__ == "__main__":
    unittest.main()
