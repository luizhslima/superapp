package dev.superapp.events;

import org.springframework.beans.factory.annotation.Value;
import org.springframework.kafka.core.KafkaTemplate;
import org.springframework.stereotype.Service;

/** Publica no tópico usando a chave para manter a ordem por entidade. */
@Service
public class EventPublisher {
    private final KafkaTemplate<String, String> kafka;
    private final String topic;

    public EventPublisher(KafkaTemplate<String, String> kafka, @Value("${events.topic}") String topic) {
        this.kafka = kafka;
        this.topic = topic;
    }

    public void publish(EventRequest e) {
        // Formato simples: "tipo|payload". Trocar por JSON/Avro quando o contrato firmar.
        kafka.send(topic, e.key(), e.type() + "|" + e.payload());
    }
}
