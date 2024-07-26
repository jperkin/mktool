## mktool

This is intended to be a utility that replaces parts of
[pkgsrc](https://github.com/NetBSD/pkgsrc/)'s mk infrastructure.

As an example, the first command to be implemented, `makesum`, already improves
the performance of generating the `distinfo` for `wip/grafana` from:

```
real    5m55.346s
user    1m11.179s
sys     3m32.786s
```

to just:

```
real    0m9.579s
user    0m7.752s
sys     0m1.759s
```

and that's before any work on optimisation.
