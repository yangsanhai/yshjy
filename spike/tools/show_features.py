import tomllib, sys, pathlib
p = pathlib.Path(sys.argv[1])
d = tomllib.loads(p.read_text())
print("features:", sorted(d["features"].keys()))
print("default =", d["features"].get("default"))
