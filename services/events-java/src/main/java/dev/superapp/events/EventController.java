package dev.superapp.events;

import java.util.List;

import org.springframework.http.HttpStatus;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestBody;
import org.springframework.web.bind.annotation.ResponseStatus;
import org.springframework.web.bind.annotation.RestController;

import jakarta.validation.Valid;

@RestController
public class EventController {
    private final EventPublisher publisher;
    private final EventConsumer consumer;

    public EventController(EventPublisher publisher, EventConsumer consumer) {
        this.publisher = publisher;
        this.consumer = consumer;
    }

    @PostMapping("/v1/events")
    @ResponseStatus(HttpStatus.ACCEPTED)
    public void publish(@Valid @RequestBody EventRequest body) {
        publisher.publish(body);
    }

    @GetMapping("/v1/events/recent")
    public List<String> recent() {
        return consumer.recent();
    }
}
