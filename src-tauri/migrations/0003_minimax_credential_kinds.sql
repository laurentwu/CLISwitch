-- MiniMax Token Plan keys have a stable sk-cp- prefix. Earlier catalog versions stored every
-- MiniMax profile as a Token Plan using X-Api-Key, so reclassify the provider identity and auth
-- type without changing connection IDs or saved-configuration references.
UPDATE provider_profiles
SET template_id = CASE
      WHEN template_id = 'minimax-coding-plan'
        AND EXISTS (
          SELECT 1
          FROM provider_connections
          WHERE provider_id = provider_profiles.id
            AND substr(api_key, 1, 6) <> 'sk-cp-'
        )
        THEN 'minimax-api'
      WHEN template_id = 'minimax-cn-coding-plan'
        AND EXISTS (
          SELECT 1
          FROM provider_connections
          WHERE provider_id = provider_profiles.id
            AND substr(api_key, 1, 6) <> 'sk-cp-'
        )
        THEN 'minimax-cn-api'
      ELSE template_id
    END,
    revision = revision + 1,
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE template_id IN ('minimax-coding-plan', 'minimax-cn-coding-plan');

UPDATE provider_connections
SET auth_type = CASE
      WHEN substr(api_key, 1, 6) = 'sk-cp-' THEN 'bearer'
      ELSE 'api-key'
    END,
    verification_status = 'user-modified-unverified',
    verified_at = NULL,
    verification_error = NULL
WHERE template_endpoint_id = 'anthropic'
  AND provider_id IN (
    SELECT id
    FROM provider_profiles
    WHERE template_id IN (
      'minimax-api',
      'minimax-cn-api',
      'minimax-coding-plan',
      'minimax-cn-coding-plan'
    )
  )
  AND auth_type <> CASE
    WHEN substr(api_key, 1, 6) = 'sk-cp-' THEN 'bearer'
    ELSE 'api-key'
  END;
