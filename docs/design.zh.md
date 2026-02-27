# miniclaw 设计

## 特性

- 持续监听来自频道的消息
- 将消息传给 claude code
- 一个 miniclaw 对应一个仓库
- miniclaw 配置文件放在 .claude 文件夹下
- 通过 hook 回复消息给频道

## toml 配置

因为只转发消息给 claude code，因此不需要配置大模型

```toml
[channel.wecom]
token = ""
encoding_aes_key = ""
```
