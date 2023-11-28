import bitbazaar
from bitbazaar.utils import add


def test_hello():
    assert bitbazaar.hello() == "Hello, World!"


def test_add():
    assert add(1, 2) == 3
