from app.util import caller
from app.models import Widget
from app.gizmo import Gizmo
from app.lefty import Lefty


def run():
    caller()
    Widget.build()


def run_bare():
    # `build` is defined on both Widget and Gadget with no qualifier here — must be
    # dropped as ambiguous, never guessed.
    build()


def run_via_variable():
    # Instance call through a variable receiver (issue #8): `spin` has exactly one definition in
    # the repo (Gizmo.spin()), so this must resolve even though the variable is named `g`, not
    # `Gizmo` — a value receiver carries no type information, and the call falls through to
    # bare-name resolution, exactly like a bare `spin()` call would.
    g = Gizmo()
    g.spin()


def run_ambiguous_via_variable():
    # Negative: two same-named methods on different classes (Lefty.orbit()/Righty.orbit()),
    # called through a variable receiver — must still be dropped as ambiguous. A value receiver
    # must not turn "drop ambiguous" into "guess".
    l = Lefty()
    l.orbit()
