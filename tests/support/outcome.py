"""module: outcome; responsibility: represent expected external operation results.

operations: Ok, Rejected, accepted, present; entities: Ok, Rejected; tests: outcome_tests.
"""

from dataclasses import dataclass


@dataclass(frozen=True)
class Ok[T]:
    value: T


@dataclass(frozen=True)
class Rejected:
    code: int
    operation: str


type Result[T] = Ok[T] | Rejected


def accepted[T](result: Result[T]) -> T:
    assert isinstance(result, Ok), result
    return result.value


def present[T](value: T | None) -> T:
    assert value is not None
    return value
