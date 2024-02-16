"""Used internally by utils.sh match_substring."""

import re
import sys

r = sys.argv[1]
s = sys.argv[2]


def err_inf():
    """Helper for error message."""
    return "\n\nString:\n{}\n\nRegex: '{}'\n\n".format(s, r)


res = re.findall(r, s)
if len(res) != 1:
    raise ValueError("{}Expected 1 match, got: {}.".format(err_inf(), len(res)))

match = res[0]
if match == "":
    raise ValueError("{}Matched empty string.".format(err_inf()))

print(match.strip())
