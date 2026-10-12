#!/usr/bin/env python3
"""Load configuration files with the config parser of Klipper or Kalico, and check that printerkeep's
"needs decision" blocks and override comments have no effect.

Usage: check_markers.py <klipper or kalico checkout> <cases folder>

Each case is three files:

  <case>.cfg         a file with needs-decision blocks, as printerkeep writes it
  <case>.mine.cfg    the same configuration without blocks: this printer's lines
  <case>.shared.cfg  the same configuration without blocks: the shared lines

The check:

1. <case>.cfg gives the same configuration as <case>.mine.cfg. The blocks change nothing.
2. Keep mine: delete the commented shared lines and all #### lines. The result gives the same
   configuration as <case>.mine.cfg.
3. Use shared: delete the active lines, remove the # from the start of the shared lines, and delete all
   #### lines (the steps that the block tells). The result gives the same configuration as
   <case>.shared.cfg.

Files in subfolders are not cases. Cases include them with [include].
"""

import importlib.util
import os
import sys
import tempfile
import types

MARKER = "####"
BLOCK_START = "#### printerkeep: needs decision"
BLOCK_END = "#### printerkeep: end"
OTHER_VERSION = ("#### Shared version.", "#### Repository version.")


def load_firmware(checkout):
    """Returns (name, function that parses a file into {section: {option: value}})."""
    klippy = os.path.join(checkout, "klippy")
    # Load configfile.py only. The klippy package loads all of the firmware.
    package = types.ModuleType("klippy")
    package.__path__ = [klippy]
    sys.modules["klippy"] = package
    spec = importlib.util.spec_from_file_location(
        "klippy.configfile", os.path.join(klippy, "configfile.py")
    )
    configfile = importlib.util.module_from_spec(spec)
    sys.modules["klippy.configfile"] = configfile
    spec.loader.exec_module(configfile)

    if hasattr(configfile, "ConfigFileReader"):
        # Klipper
        reader = configfile.ConfigFileReader()
        find_autosave = configfile.ConfigAutoSave._find_autosave_data

        def parse(filename):
            data = reader.read_config_file(filename)
            regular, autosave = find_autosave(None, data)
            fileconfigs = [reader.build_fileconfig_with_includes(regular, filename)]
            fileconfigs.append(reader.build_fileconfig(autosave, filename))
            return regular, autosave, fileconfigs

        return "klipper", parse

    # Kalico
    import configparser

    printer_config = configfile.PrinterConfig.__new__(configfile.PrinterConfig)

    def new_fileconfig():
        return configparser.RawConfigParser(
            strict=False,
            inline_comment_prefixes=(";", "#"),
            interpolation=configfile.SectionInterpolation({}),
        )

    def parse(filename):
        data = printer_config._read_config_file(filename)
        regular, autosave = printer_config._find_autosave_data(data)
        fileconfigs = [new_fileconfig(), new_fileconfig()]
        printer_config._parse_config(regular, filename, fileconfigs[0], set())
        printer_config._parse_config(autosave, filename, fileconfigs[1], set())
        return regular, autosave, fileconfigs

    return "kalico", parse


def configuration(parse, filename):
    """The configuration that the firmware uses. Empty lines in values are removed: in a gcode
    template, an empty line does nothing."""
    regular, autosave, fileconfigs = parse(filename)
    if "#*#" in regular:
        raise AssertionError(f"{filename}: the firmware did not read the SAVE_CONFIG part")
    result = {}
    for fileconfig in fileconfigs:
        for section in fileconfig.sections():
            options = result.setdefault(section, {})
            for option, value in fileconfig.items(section, raw=True):
                lines = [line.rstrip() for line in value.split("\n")]
                options[option] = [line for line in lines if line]
    return result


def decide(text, use_shared):
    """Do what a user does with each block: keep this printer's lines, or use the shared lines."""
    out = []
    state = "outside"
    for line in text.split("\n"):
        if state == "outside":
            if line.startswith(BLOCK_START):
                state = "active"
            else:
                out.append(line)
        elif line.startswith(BLOCK_END):
            state = "outside"
        elif line.startswith(OTHER_VERSION):
            state = "shared"
        elif line.startswith(MARKER):
            pass
        elif state == "active":
            if not use_shared:
                out.append(line)
        else:
            if not line.startswith("#"):
                raise AssertionError(f"shared line without #: {line!r}")
            if use_shared:
                out.append(line[1:])
    if state != "outside":
        raise AssertionError("a block has no end line")
    return "\n".join(out)


def check_case(parse, cases, case):
    path = os.path.join(cases, case + ".cfg")
    mine = configuration(parse, os.path.join(cases, case + ".mine.cfg"))
    shared = configuration(parse, os.path.join(cases, case + ".shared.cfg"))
    if mine == shared:
        raise AssertionError(f"{case}: the mine and shared files must be different")
    problems = []
    if configuration(parse, path) != mine:
        problems.append("with blocks, the configuration is not the same as .mine.cfg")
    with open(path) as f:
        text = f.read()
    for use_shared, expected, name in ((False, mine, "keep mine"), (True, shared, "use shared")):
        # In the same folder, so that [include] finds the same files.
        with tempfile.NamedTemporaryFile(
            "w", dir=cases, prefix=".decided-", suffix=".cfg", delete=False
        ) as f:
            f.write(decide(text, use_shared))
        try:
            if configuration(parse, f.name) != expected:
                problems.append(f"after '{name}', the configuration is not as expected")
        finally:
            os.unlink(f.name)
    return problems


def main():
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    checkout, cases = sys.argv[1], sys.argv[2]
    name, parse = load_firmware(checkout)
    names = sorted(
        f[: -len(".cfg")]
        for f in os.listdir(cases)
        if f.endswith(".cfg") and not f.endswith((".mine.cfg", ".shared.cfg"))
        and not f.startswith(".")
    )
    if not names:
        sys.exit(f"no cases in {cases}")
    failed = False
    for case in names:
        problems = check_case(parse, cases, case)
        print(f"{name}: {case}: {'ok' if not problems else 'FAILED'}")
        for problem in problems:
            print(f"  {problem}")
        failed = failed or bool(problems)
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
