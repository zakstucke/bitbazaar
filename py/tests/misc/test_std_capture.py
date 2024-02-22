import sys

from bitbazaar.misc import StdCapture


def test_std_capture():
    orig_stdout = sys.stdout
    orig_stderr = sys.stderr

    # Confirm the example works:
    with StdCapture(stderr=True) as out:  # By default only captures stdout
        print("hello")
        sys.stderr.write("world")
    assert out == ["hello", "world"]

    # Confirm no stderr captured if not requested:
    with StdCapture() as out:
        sys.stderr.write("world")
    assert out == []

    # Confirm returned to originals:
    assert sys.stdout is orig_stdout
    assert sys.stderr is orig_stderr

    # Confirm would also return if error occurred inside block:
    try:
        with StdCapture(stderr=True):
            raise ValueError("error")
    except ValueError:
        pass

    assert sys.stdout is orig_stdout
    assert sys.stderr is orig_stderr
