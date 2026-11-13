package dev.superapp.events;

import java.util.List;
import java.util.concurrent.CopyOnWriteArrayList;

import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.kafka.annotation.KafkaListener;
import org.springframework.stereotype.Component;

/** Consumidor de exemplo: guarda os últimos eventos em memória (casca). */
@Component
public class EventConsumer {
    private static final Logger log = LoggerFactory.getLogger(EventConsumer.class);
    private static final int KEEP = 100;

    private final List<String> recent = new CopyOnWriteArrayList<>();

    @KafkaListener(topics = "${events.topic}")
    public void on(String message) {
        log.info("evento recebido ({} bytes)", message.length()); // não loga o conteúdo
        recent.add(message);
        while (recent.size() > KEEP) {
            recent.remove(0);
        }
    }

    public List<String> recent() {
        return List.copyOf(recent);
    }
}
