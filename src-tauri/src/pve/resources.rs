use crate::error::AppResult;
use crate::pve::client::PveClient;
use crate::pve::models::{ClusterResource, Guest, Realm, VmConfig};

/// 匿名探活 + 拉 realm 列表。
///
/// 重要：PVE 上 `/version` 与 `/cluster/resources` 都要求认证（实测返回 401），
/// 唯一允许匿名访问的是 `/access/domains`（源码中声明 user => 'world'，
/// 注释写明是给登录前的登录框用的）。因此探活以它为准。
pub async fn probe(client: &PveClient) -> AppResult<(String, Vec<Realm>)> {
    let realms: Vec<Realm> = client.get_anonymous("access/domains").await?;

    // 能读到 realm 列表即说明地址正确且是 PVE。版本号要登录后才能取，
    // 此处先留空，登录成功后再补。
    Ok((client.version(), realms))
}

/// 拉取当前用户有权限的虚拟机。
///
/// `/cluster/resources` 内部已按 `VM.Audit` 逐台过滤，客户端无需再做权限判断；
/// 这里只过滤掉模板，并把 tags/lock 等字段整理成前端更易用的形状。
pub async fn list_guests(client: &PveClient) -> AppResult<Vec<Guest>> {
    let resources: Vec<ClusterResource> = client.get("cluster/resources?type=vm").await?;

    let mut guests: Vec<Guest> = resources
        .into_iter()
        .filter(|r| r.kind == "qemu" || r.kind == "lxc")
        .filter(|r| r.template.unwrap_or(0) == 0)
        .map(|r| Guest {
            vmid: r.vmid,
            node: r.node,
            kind: r.kind,
            name: r
                .name
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| format!("VM {}", r.vmid)),
            status: r.status.unwrap_or_else(|| "unknown".to_string()),
            pool: r.pool,
            tags: r
                .tags
                .map(|t| {
                    t.split([';', ','])
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_default(),
            cpu: r.cpu,
            maxcpu: r.maxcpu,
            mem: r.mem,
            maxmem: r.maxmem,
            uptime: r.uptime,
            lock: r.lock,
            hastate: r.hastate,
            spice_capable: false,
            vga: None,
        })
        .collect();

    guests.sort_by_key(|g| g.vmid);

    // 判定 SPICE 能力：只对 QEMU 有意义，LXC 的 spiceproxy 只是把文本终端塞进 SPICE。
    // config 端点只需 VM.Audit，落在列表权限范围内。
    let spice_flags = futures_util::future::join_all(guests.iter().map(|g| async {
        if g.kind != "qemu" {
            return None;
        }
        fetch_vm_config(client, &g.node, g.vmid)
            .await
            .ok()
            .map(|c| (c.supports_spice(), c.vga.clone()))
    }))
    .await;

    for (guest, flag) in guests.iter_mut().zip(spice_flags) {
        if let Some((capable, vga)) = flag {
            guest.spice_capable = capable;
            guest.vga = vga;
        }
    }

    Ok(guests)
}

async fn fetch_vm_config(
    client: &PveClient,
    node: &str,
    vmid: u32,
) -> AppResult<VmConfig> {
    client
        .get(&format!("nodes/{node}/qemu/{vmid}/config"))
        .await
}
