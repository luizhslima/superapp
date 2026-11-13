package dev.superapp.events;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import org.junit.jupiter.api.Test;

import jakarta.validation.Validation;
import jakarta.validation.Validator;

class EventRequestTest {
    private final Validator v = Validation.buildDefaultValidatorFactory().getValidator();

    @Test
    void acceptsValid() {
        assertTrue(v.validate(new EventRequest("order.created", "o1", "{}")).isEmpty());
    }

    @Test
    void rejectsBadTypeBlankAndHuge() {
        assertEquals(1, v.validate(new EventRequest("Order Created!", "o1", "{}")).size());
        assertEquals(1, v.validate(new EventRequest("a", " ", "{}")).size());
        assertEquals(1, v.validate(new EventRequest("a", "k", "x".repeat(16_001))).size());
    }
}
