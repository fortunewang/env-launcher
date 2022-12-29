# env-launcher

```toml
# another-python.toml
[env]
PATH = { sep = ';', prepand = ['C:\path\to\another\python']}
```

```shell
env-launcher.exe -c another-python.toml -- python.exe
```
