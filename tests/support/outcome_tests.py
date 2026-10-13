"""module: tests.support.outcome_tests; responsibility: define the tested public contract.

operations: pytest cases; entities: none; tests:
    test_accepted_preserves_typed_value
    test_rejected_preserves_errno_and_operation


"""

from tests.support.outcome import Ok, Rejected, accepted


def test_accepted_preserves_typed_value() -> None:
    assert accepted(Ok((1, "sample"))) == (1, "sample")


def test_rejected_preserves_errno_and_operation() -> None:
    result = Rejected(22, "ioctl")
    assert (result.code, result.operation) == (22, "ioctl")
