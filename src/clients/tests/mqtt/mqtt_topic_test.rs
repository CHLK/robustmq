#[cfg(test)]
mod tests {
    use crate::common::get_placement_addr;
    use bytes::Bytes;
    use clients::{
        placement::mqtt::call::{placement_create_topic, placement_list_topic,
            placement_delete_topic, placement_set_topic_retain_message},
        poll::ClientPool,
    };
    use metadata_struct::mqtt::{message::MQTTMessage, topic::MQTTTopic};
    use protocol::{mqtt::common::{qos, Publish}, placement_center::generate::mqtt::{
        CreateTopicRequest, DeleteTopicRequest, ListTopicRequest, SetTopicRetainMessageRequest
    }};
    use std::sync::Arc;

    #[tokio::test]
    async fn mqtt_topic_test() {
        let client_poll: Arc<ClientPool> = Arc::new(ClientPool::new(3));
        let addrs = vec![get_placement_addr()];
        let client_id: String = "test_cient_id".to_string();
        let topic_id: String = "test_topic_ic".to_string();
        let topic_name: String = "test_topic".to_string();
        let cluster_name: String = "test_cluster".to_string();
        let payload: String = "test_message".to_string();
        let retain_message_expired_at: u64 = 10000;
        
        let mut mqtt_topic: MQTTTopic = MQTTTopic {
            topic_id: topic_id.clone(),
            topic_name: topic_name.clone(),
            retain_message: None,
            retain_message_expired_at: None,
        };
        
        let request = CreateTopicRequest {
            cluster_name: cluster_name.clone(),
            topic_name: mqtt_topic.topic_name.clone(),
            content: mqtt_topic.encode(),
        };
        match placement_create_topic(client_poll.clone(), addrs.clone(), request).await {
            Ok(_) => {},
            Err(e) => {
                println!("{:?}", e);
                assert!(false);
            }
        }

        let request = ListTopicRequest {
            cluster_name: cluster_name.clone(),
            topic_name: mqtt_topic.topic_name.clone(),
        };
        match placement_list_topic(client_poll.clone(), addrs.clone(), request).await {
            Ok(data) => {
                assert!(!data.topics.is_empty());
                let mut flag: bool = false;
                for raw in data.topics {
                    let topic = serde_json::from_slice::<MQTTTopic>(raw.as_slice()).unwrap();
                    if topic == mqtt_topic {
                        flag = true;
                    }
                }
                assert!(flag);
            },
            Err(e) => {
                println!("{:?}", e);
                assert!(false);
            }
        }
        
        let publish: Publish = Publish {
            dup: false,
            qos: qos(1).unwrap(),
            pkid: 0,
            retain: true,
            topic: Bytes::from(topic_name.clone()),
            payload: Bytes::from(payload.clone()),
        };
        let retain_message = MQTTMessage::build_message(&client_id, &publish, &None);
        mqtt_topic = MQTTTopic {
            topic_id: topic_id.clone(),
            topic_name: topic_name.clone(),
            retain_message: Some(retain_message.encode()),
            retain_message_expired_at: Some(retain_message_expired_at.clone()),
        };

        let request = SetTopicRetainMessageRequest {
            cluster_name: cluster_name.clone(),
            topic_name: mqtt_topic.topic_name.clone(),
            retain_message: mqtt_topic.retain_message.clone().unwrap(),
            retain_message_expired_at: mqtt_topic.retain_message_expired_at.clone().unwrap(),
        };
        match placement_set_topic_retain_message(client_poll.clone(), addrs.clone(), request).await {
            Ok(_) => {},
            Err(e) => {
                println!("{:?}", e);
                assert!(false);
            }
        }

        let request = ListTopicRequest {
            cluster_name: cluster_name.clone(),
            topic_name: mqtt_topic.topic_name.clone(),
        };
        match placement_list_topic(client_poll.clone(), addrs.clone(), request).await {
            Ok(data) => {
                assert!(!data.topics.is_empty());
                let mut flag: bool = false;
                for raw in data.topics {
                    let topic = serde_json::from_slice::<MQTTTopic>(raw.as_slice()).unwrap();
                    if topic == mqtt_topic {
                        flag = true;
                    }
                }
                assert!(flag);
            },
            Err(e) => {
                println!("{:?}", e);
                assert!(false);
            }
        }

        let request = DeleteTopicRequest {
            cluster_name: cluster_name.clone(),
            topic_name: mqtt_topic.topic_name.clone(),
        };
        match placement_delete_topic(client_poll.clone(), addrs.clone(), request).await {
            Ok(_) => {},
            Err(e) => {
                println!("{:?}", e);
                assert!(false);
            }
        }

        let request = ListTopicRequest {
            cluster_name: cluster_name.clone(),
            topic_name: mqtt_topic.topic_name.clone(),
        };
        match placement_list_topic(client_poll.clone(), addrs.clone(), request).await {
            Ok(data) => {
                assert!(data.topics.is_empty());
                let mut flag: bool = false;
                for raw in data.topics {
                    let topic = serde_json::from_slice::<MQTTTopic>(raw.as_slice()).unwrap();
                    if topic == mqtt_topic {
                        flag = true;
                    }
                }
                assert!(!flag);
            },
            Err(e) => {
                println!("{:?}", e);
                assert!(false);
            }
        }

    }
}