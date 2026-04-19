use std::net::SocketAddr;

use crate::{
    common::global_ctx::ArcGlobalCtx,
    proto::{
        common::Void,
        peer_rpc::{
            DirectConnectorRpc, GetIpListRequest, GetIpListResponse, SendUdpHolePunchPacketRequest,
        },
        rpc_types::{self, controller::BaseController},
    },
    tunnel::udp,
};

#[derive(Clone)]
pub struct DirectConnectorManagerRpcServer {
    // TODO: this only cache for one src peer, should make it global
    global_ctx: ArcGlobalCtx,
}

#[async_trait::async_trait]
impl DirectConnectorRpc for DirectConnectorManagerRpcServer {
    type Controller = BaseController;

    async fn get_ip_list(
        &self,
        _: BaseController,
        _: GetIpListRequest,
    ) -> rpc_types::error::Result<GetIpListResponse> {
        let mut ret = self.global_ctx.get_ip_collector().collect_ip_addrs().await;

        // Build a set of (scheme, port) pairs that belong to rproxy listeners so we
        // can exclude them from the advertised listener list.  Peers should never be
        // told to dial these addresses directly – they are only reachable through the
        // reverse proxy, and advertising them would just route peers through the proxy
        // again instead of attempting a real P2P connection.
        let rproxy_keys: std::collections::HashSet<(String, u16)> = self
            .global_ctx
            .config
            .get_rproxy_listeners()
            .iter()
            .filter_map(|u| u.port().map(|p| (u.scheme().to_string(), p)))
            .collect();

        ret.listeners = self
            .global_ctx
            .config
            .get_mapped_listeners()
            .into_iter()
            .chain(self.global_ctx.get_running_listeners())
            .filter(|u| {
                // Keep the URL unless it matches one of the rproxy (scheme, port) pairs.
                u.port()
                    .map(|p| !rproxy_keys.contains(&(u.scheme().to_string(), p)))
                    .unwrap_or(true)
            })
            .map(Into::into)
            .collect();
        // remove et ipv6 from the interface ipv6 list
        if let Some(et_ipv6) = self.global_ctx.get_ipv6() {
            let et_ipv6: crate::proto::common::Ipv6Addr = et_ipv6.address().into();
            ret.interface_ipv6s.retain(|x| *x != et_ipv6);
        }
        tracing::trace!(
            "get_ip_list: public_ipv4: {:?}, public_ipv6: {:?}, listeners: {:?}",
            ret.public_ipv4,
            ret.public_ipv6,
            ret.listeners
        );
        Ok(ret)
    }

    async fn send_udp_hole_punch_packet(
        &self,
        _: BaseController,
        req: SendUdpHolePunchPacketRequest,
    ) -> rpc_types::error::Result<Void> {
        let listener_port = req.listener_port as u16;
        let connector_addr: SocketAddr = req
            .connector_addr
            .ok_or(anyhow::anyhow!("connector_addr is required"))?
            .into();

        tracing::info!(
            "Sending udp hole punch packet to {} from listener port {}",
            connector_addr,
            listener_port
        );

        // send 3 packets to the connector
        for _ in 0..3 {
            match connector_addr {
                SocketAddr::V4(addr) => udp::send_v4_hole_punch_packet(listener_port, addr).await?,
                SocketAddr::V6(addr) => udp::send_v6_hole_punch_packet(listener_port, addr).await?,
            }
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        }
        Ok(Default::default())
    }
}

impl DirectConnectorManagerRpcServer {
    pub fn new(global_ctx: ArcGlobalCtx) -> Self {
        Self { global_ctx }
    }
}
