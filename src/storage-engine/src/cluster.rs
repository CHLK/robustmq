use clients::placement_center::{register_node, unregister_node};
use common::{config::storage_engine::StorageEngineConfig, log::info_meta};
use protocol::placement_center::placement::{
    ClusterType, RegisterNodeRequest, UnRegisterNodeRequest,
};

pub async fn register_storage_engine_node(config: StorageEngineConfig) {
    let mut req = RegisterNodeRequest::default();
    req.cluster_type = ClusterType::StorageEngine.into();
    req.cluster_name = config.cluster_name;
    req.node_id = config.node_id;
    req.node_ip = config.addr;
    req.node_port = config.port;
    req.extend_info = "".to_string();

    let mut res_err = None;
    for addr in config.placement_center {
        match register_node(&addr, req.clone()).await {
            Ok(_) => {
                info_meta(&format!(
                    "Node {} has been successfully registered",
                    config.node_id
                ));
                break;
            }
            Err(e) => {
                res_err = Some(e);
            }
        }
    }
    if !res_err.is_none() {
        let info = res_err.unwrap().to_string();
        panic!("{}", info);
    }
}

pub async fn unregister_storage_engine_node(config: StorageEngineConfig) {
    let mut req = UnRegisterNodeRequest::default();
    req.cluster_type = ClusterType::StorageEngine.into();
    req.cluster_name = config.cluster_name;
    req.node_id = config.node_id;

    let mut res_err = None;
    for addr in config.placement_center {
        match unregister_node(&addr, req.clone()).await {
            Ok(_) => {
                info_meta(&format!(
                    "Node {} exits successfully",
                    config.node_id
                ));
                break;
            }
            Err(e) => {
                res_err = Some(e);
            }
        }
    }
    if !res_err.is_none() {
        let info = res_err.unwrap().to_string();
        panic!("{}", info);
    }
}