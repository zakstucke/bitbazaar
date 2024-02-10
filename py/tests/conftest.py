import pytest
from bitbazaar._testing import mark_testing


@pytest.fixture(scope="session", autouse=True)
def setup_before_tests():
    mark_testing()
