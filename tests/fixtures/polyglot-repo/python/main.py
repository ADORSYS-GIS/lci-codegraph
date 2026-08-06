from python.util import caller
from python.models import Widget


def run():
    caller()
    Widget.build()


def run_bare():
    # `build` is defined on both Widget and Gadget with no qualifier here — must be
    # dropped as ambiguous, never guessed.
    build()
