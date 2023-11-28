import bitbazaar_rs


def test_hello():
    assert bitbazaar_rs.hello() == "Hello, World!"


def test_add():
    assert bitbazaar_rs.utils.add(1, 2) == 3
