package dev.superapp.events;

import jakarta.validation.constraints.NotBlank;
import jakarta.validation.constraints.Pattern;
import jakarta.validation.constraints.Size;

/** Entrada da API. Limites explícitos: o corpo é dado não confiável. */
public record EventRequest(
        @NotBlank @Size(max = 100) @Pattern(regexp = "[a-z0-9._-]+") String type,
        @NotBlank @Size(max = 100) String key,
        @NotBlank @Size(max = 16_000) String payload) {
}
