import unittest

from verify_instrumentation import verify


def result():
    lines = []
    for current in (1, 2):
        for code in (1, 0):
            lines.extend(["INSTRUMENTATION_STATUS: class=ExampleTest",
                          f"INSTRUMENTATION_STATUS: test=case{current}",
                          f"INSTRUMENTATION_STATUS: current={current}",
                          "INSTRUMENTATION_STATUS: numtests=2",
                          f"INSTRUMENTATION_STATUS_CODE: {code}"])
    return "\n".join(lines) + "\nOK (2 tests)\nINSTRUMENTATION_CODE: -1\n"


class InstrumentationVerification(unittest.TestCase):
    def test_complete_protocol_preserves_both_test_identities(self):
        receipt = verify(result(), 2)
        self.assertEqual(receipt["tests_passed"], 2)
        self.assertEqual([test["method"] for test in receipt["tests"]], ["case1", "case2"])

    def test_failures_skips_truncation_duplicates_and_false_summaries_are_rejected(self):
        good = result()
        for bad in [
            "OK (2 tests)\nINSTRUMENTATION_CODE: -1\n",
            good.replace("INSTRUMENTATION_STATUS_CODE: 0", "INSTRUMENTATION_STATUS_CODE: -2", 1),
            good.replace("INSTRUMENTATION_STATUS_CODE: 0", "INSTRUMENTATION_STATUS_CODE: -3", 1),
            good.replace("INSTRUMENTATION_CODE: -1", "INSTRUMENTATION_CODE: 0"),
            good.replace("INSTRUMENTATION_CODE: -1", ""),
            good.replace("OK (2 tests)", "FAILURES!!!"),
            good.replace("numtests=2", "numtests=3", 1),
            good.replace("current=2", "current=1"),
            good.replace("case2", "case1"),
            good + "OK (2 tests)\n",
            good.replace("INSTRUMENTATION_STATUS_CODE: 0\n", "", 1),
        ]:
            with self.subTest(output=bad), self.assertRaises(ValueError):
                verify(bad, 2)
        with self.assertRaises(ValueError):
            verify(good, 23)


if __name__ == "__main__":
    unittest.main()
