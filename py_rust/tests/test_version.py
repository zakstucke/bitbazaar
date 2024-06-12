import bitbazaar_rs


def test_version():
    """Just a default example version test."""
    from importlib.metadata import version

    assert bitbazaar_rs.__version__ == version("bitbazaar_rs")
