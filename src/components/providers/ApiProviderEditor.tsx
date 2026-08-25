import { useEffect, useMemo, useRef, useState } from "react";
import { zodResolver } from "@hookform/resolvers/zod";
import { useFieldArray, useForm, useWatch } from "react-hook-form";
import { z } from "zod";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Copy, Files, Plus, Save, Trash2, Wifi } from "lucide-react";
import { useTranslation } from "react-i18next";
import { apiTemplate } from "../../shared/catalog";
import { command, errorMessage } from "../../shared/ipc";
import { uniqueCopyName, validateEntityName } from "../../shared/names";
import type {
  ApiProviderDetail,
  ApiProviderDraft,
  ApiProviderTemplate,
  ProviderCatalog,
  ProviderEndpointTemplate,
  PublicProvider,
} from "../../shared/types";
import { useUiStore } from "../../stores/ui";
import { Badge, Button, Card, Field, Input, Select } from "../ui";

const connectionSchema = z.object({
  id: z.string().optional(),
  templateEndpointId: z.string().optional(),
  credentialSlotId: z.string().trim().min(1),
  protocol: z.enum(["openai-chat", "openai-responses", "anthropic-messages"]),
  endpoint: z
    .url()
    .refine((value) => ["http:", "https:"].includes(new URL(value).protocol), "HTTP(S) required"),
  authType: z.enum(["api-key", "bearer"]),
  apiKey: z.string().min(1),
  defaultModel: z.string().trim().min(1),
});

const schema = z
  .object({
    name: z.string().refine((value) => {
      const length = [...value.trim()].length;
      return length >= 1 && length <= 64;
    }),
    templateId: z.string().optional(),
    connections: z.array(connectionSchema).min(1),
  })
  .superRefine((value, context) => {
    const slotSecrets = new Map<string, string>();
    value.connections.forEach((connection, index) => {
      if (connection.protocol !== "anthropic-messages" && connection.authType !== "bearer")
        context.addIssue({
          code: "custom",
          message: "OpenAI-compatible protocols require bearer authentication",
          path: ["connections", index, "authType"],
        });
      const previous = slotSecrets.get(connection.credentialSlotId);
      if (previous !== undefined && previous !== connection.apiKey)
        context.addIssue({
          code: "custom",
          message: "Connections in one credential slot must share a key",
          path: ["connections", index, "apiKey"],
        });
      slotSecrets.set(connection.credentialSlotId, connection.apiKey);
    });
  });

type DraftConnection = ApiProviderDraft["connections"][number];

function defaultAuth(endpoint: ProviderEndpointTemplate) {
  return (
    endpoint.authOptions.find((option) => option.id === endpoint.defaultAuthOptionId)?.authType ??
    endpoint.authOptions[0]?.authType ??
    "bearer"
  );
}

function defaultModel(endpoint: ProviderEndpointTemplate) {
  return endpoint.models.find((model) => model.default)?.id ?? endpoint.models[0]?.id ?? "";
}

function templateConnections(
  template: ApiProviderTemplate,
  existing: DraftConnection[],
): DraftConnection[] {
  const secrets = new Map<string, string>();
  for (const connection of existing) {
    if (connection.apiKey) secrets.set(connection.credentialSlotId, connection.apiKey);
  }
  return template.endpoints.map((endpoint) => {
    return {
      templateEndpointId: endpoint.id,
      credentialSlotId: endpoint.credentialSlotId,
      protocol: endpoint.protocol,
      endpoint: endpoint.baseUrl,
      authType: defaultAuth(endpoint),
      apiKey: secrets.get(endpoint.credentialSlotId) ?? "",
      defaultModel: defaultModel(endpoint),
    };
  });
}

function reconcileTemplateConnections(
  template: ApiProviderTemplate,
  existing: DraftConnection[],
): DraftConnection[] {
  const bySlot = new Map<string, string>();
  for (const connection of existing) {
    if (connection.apiKey) bySlot.set(connection.credentialSlotId, connection.apiKey);
  }
  return template.endpoints.map((endpoint) => {
    const previous = existing.find((connection) => connection.templateEndpointId === endpoint.id);
    const previousAuthIsValid = endpoint.authOptions.some(
      (option) => option.authType === previous?.authType,
    );
    return {
      id: previous?.id,
      templateEndpointId: endpoint.id,
      credentialSlotId: endpoint.credentialSlotId,
      protocol: endpoint.protocol,
      endpoint: previous?.endpoint ?? endpoint.baseUrl,
      authType: previousAuthIsValid && previous ? previous.authType : defaultAuth(endpoint),
      apiKey: bySlot.get(endpoint.credentialSlotId) ?? previous?.apiKey ?? "",
      defaultModel: previous?.defaultModel ?? defaultModel(endpoint),
    };
  });
}

function normalizeDraft(value: ApiProviderDraft): ApiProviderDraft {
  const secrets = new Map<string, string>();
  for (const connection of value.connections) {
    if (!secrets.has(connection.credentialSlotId)) {
      secrets.set(connection.credentialSlotId, connection.apiKey);
    }
  }
  return {
    ...value,
    name: value.name.trim(),
    templateId: value.templateId || undefined,
    connections: value.connections.map((connection) => ({
      ...connection,
      apiKey: secrets.get(connection.credentialSlotId) ?? connection.apiKey,
      templateEndpointId: value.templateId ? connection.templateEndpointId : undefined,
    })),
  };
}

export function ApiProviderEditor({
  detail,
  providers,
  catalog,
  onClose,
  onError,
}: {
  detail?: ApiProviderDetail;
  providers: PublicProvider[];
  catalog: ProviderCatalog;
  onClose: () => void;
  onError: (message: string) => void;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const setDirty = useUiStore((state) => state.setDirty);
  const setSaveCurrent = useUiStore((state) => state.setSaveCurrent);
  const [notice, setNotice] = useState<string>();
  const [modelOptions, setModelOptions] = useState<Record<string, string[]>>({});
  const templates = useMemo(
    () =>
      catalog.providerTemplates.filter(
        (template): template is ApiProviderTemplate => template.mode === "api",
      ),
    [catalog],
  );
  const detailConnections = detail?.connections.map((connection) => ({
    id: connection.id,
    templateEndpointId: connection.templateEndpointId ?? undefined,
    credentialSlotId: connection.credentialSlotId,
    protocol: connection.protocol,
    endpoint: connection.endpoint,
    authType: connection.authType,
    apiKey: connection.apiKey,
    defaultModel: connection.defaultModel,
  }));
  const detailTemplate = apiTemplate(catalog, detail?.templateId);
  const form = useForm<ApiProviderDraft>({
    resolver: zodResolver(schema),
    defaultValues: detail
      ? {
          name: detail.name,
          templateId: detail.templateId ?? "",
          connections:
            detailTemplate && detailConnections
              ? reconcileTemplateConnections(detailTemplate, detailConnections)
              : detailConnections,
        }
      : {
          name: "",
          templateId: "",
          connections: [
            {
              credentialSlotId: "custom-api-key-1",
              protocol: "openai-responses",
              endpoint: "https://api.example.com/v1",
              authType: "bearer",
              apiKey: "",
              defaultModel: "",
            },
          ],
        },
  });
  const fields = useFieldArray({ control: form.control, name: "connections" });
  const templateId = useWatch({ control: form.control, name: "templateId" });
  const watchedConnections = useWatch({ control: form.control, name: "connections" });
  const connections = useMemo(() => watchedConnections ?? [], [watchedConnections]);
  const providerName = useWatch({ control: form.control, name: "name" });
  const selectedTemplate = apiTemplate(catalog, templateId);
  const nameIssue = validateEntityName(providerName, providers, detail?.id);

  useEffect(() => {
    connections.forEach((connection, index) => {
      if (connection.protocol !== "anthropic-messages" && connection.authType !== "bearer") {
        form.setValue(`connections.${index}.authType`, "bearer", { shouldDirty: true });
      }
    });
  }, [connections, form]);
  useEffect(() => () => form.reset(), [form]);

  const save = useMutation({
    mutationFn: (draft: ApiProviderDraft) =>
      detail
        ? command<PublicProvider>("update_provider", {
            providerId: detail.id,
            expectedRevision: detail.revision,
            draft,
          })
        : command<PublicProvider>("create_provider", { draft }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["providers"] });
      setDirty(false);
      onClose();
    },
    onError: (error) => onError(errorMessage(error)),
  });
  useEffect(() => setDirty(form.formState.isDirty), [form.formState.isDirty, setDirty]);
  const saveCurrentRef = useRef<() => Promise<boolean>>(async () => false);
  useEffect(() => {
    saveCurrentRef.current = async () => {
      if (!(await form.trigger()) || nameIssue) return false;
      try {
        await save.mutateAsync(normalizeDraft(form.getValues()));
        return true;
      } catch {
        return false;
      }
    };
  });
  useEffect(() => {
    const saveCurrent = () => saveCurrentRef.current();
    setSaveCurrent(saveCurrent);
    return () => setSaveCurrent(undefined);
  }, [setSaveCurrent]);

  const duplicate = useMutation({
    mutationFn: () => {
      const value = normalizeDraft(form.getValues());
      return command<PublicProvider>("create_provider", {
        draft: {
          ...value,
          name: uniqueCopyName(value.name, t("common.duplicate"), providers),
          connections: value.connections.map((connection) => {
            const clone = { ...connection };
            delete clone.id;
            return clone;
          }),
        },
      });
    },
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["providers"] });
      setNotice(t("common.duplicate"));
    },
    onError: (error) => onError(errorMessage(error)),
  });

  const chooseTemplate = (nextTemplateId: string) => {
    if (!nextTemplateId) {
      form.setValue("templateId", "", { shouldDirty: true });
      fields.replace(
        form.getValues("connections").map((connection, index) => ({
          ...connection,
          templateEndpointId: undefined,
          credentialSlotId: connection.credentialSlotId || `custom-api-key-${index + 1}`,
        })),
      );
      return;
    }
    const template = apiTemplate(catalog, nextTemplateId);
    if (!template) return;
    form.setValue("templateId", nextTemplateId, { shouldDirty: true });
    fields.replace(templateConnections(template, form.getValues("connections")));
    if (!form.getValues("name").trim()) {
      form.setValue("name", template.name, { shouldDirty: true });
    }
    setModelOptions({});
  };

  const test = async (connectionId?: string) => {
    if (!detail || !connectionId) {
      setNotice(t("providers.saveBeforeTest"));
      return;
    }
    try {
      await command("test_connection", { providerId: detail.id, connectionId });
      setNotice(t("providers.testSucceeded"));
    } catch (error) {
      onError(errorMessage(error));
    } finally {
      await queryClient.invalidateQueries({ queryKey: ["providers"] });
      await queryClient.invalidateQueries({ queryKey: ["provider-secret", detail.id] });
    }
  };

  const loadModels = async (connectionId: string | undefined, index: number, fieldId: string) => {
    if (!detail || !connectionId) {
      setNotice(t("providers.saveBeforeModels"));
      return;
    }
    try {
      const values = await command<string[]>("list_models", {
        providerId: detail.id,
        connectionId,
      });
      setModelOptions((current) => ({ ...current, [fieldId]: values }));
      setNotice(values.join(", "));
      if (!form.getValues(`connections.${index}.defaultModel`) && values[0])
        form.setValue(`connections.${index}.defaultModel`, values[0], { shouldDirty: true });
    } catch (error) {
      onError(errorMessage(error));
    }
  };

  return (
    <form
      className="editor"
      onSubmit={form.handleSubmit((value) => {
        if (!nameIssue) save.mutate(normalizeDraft(value));
      })}
    >
      <header className="editor-header">
        <h2>{detail ? detail.name : t("providers.addApi")}</h2>
        <div className="section-actions">
          {detail ? (
            <Button
              variant="secondary"
              type="button"
              disabled={duplicate.isPending}
              onClick={() => duplicate.mutate()}
            >
              <Files size={16} /> {t("common.duplicate")}
            </Button>
          ) : null}
          <Button variant="ghost" type="button" onClick={onClose}>
            {t("common.cancel")}
          </Button>
          <Button type="submit" disabled={save.isPending || Boolean(nameIssue)}>
            <Save size={16} /> {t("common.save")}
          </Button>
        </div>
      </header>
      <div className="form-grid two-columns">
        <Field
          label={t("providers.name")}
          hint={
            nameIssue
              ? t(nameIssue === "length" ? "validation.nameLength" : "validation.nameDuplicate")
              : undefined
          }
        >
          <Input {...form.register("name")} aria-invalid={Boolean(form.formState.errors.name)} />
        </Field>
        <Field label={t("providers.template")} hint={t("providers.templateHint")}>
          <Select value={templateId ?? ""} onChange={(event) => chooseTemplate(event.target.value)}>
            <option value="">{t("providers.customTemplate")}</option>
            {templates.map((template) => (
              <option key={template.id} value={template.id}>
                {template.name}
              </option>
            ))}
          </Select>
        </Field>
      </div>
      <div className="connection-list">
        {fields.fields.map((field, index) => {
          const connection = connections[index];
          const endpointTemplate = selectedTemplate?.endpoints.find(
            (endpoint) => endpoint.id === connection?.templateEndpointId,
          );
          const verification = detail?.connections.find(
            (item) => item.id === form.getValues(`connections.${index}.id`),
          )?.verification;
          const isFirstForSlot =
            connections.findIndex(
              (item) => item.credentialSlotId === connection?.credentialSlotId,
            ) === index;
          const suggestions = [
            ...(endpointTemplate?.models.map((model) => model.id) ?? []),
            ...(modelOptions[field.id] ?? []),
          ].filter((model, modelIndex, all) => all.indexOf(model) === modelIndex);
          return (
            <Card key={field.id}>
              <input type="hidden" {...form.register(`connections.${index}.templateEndpointId`)} />
              <input type="hidden" {...form.register(`connections.${index}.credentialSlotId`)} />
              {!isFirstForSlot ? (
                <input type="hidden" {...form.register(`connections.${index}.apiKey`)} />
              ) : null}
              <div className="card-title-row">
                <div>
                  <h3>
                    {endpointTemplate?.name ?? `${t("providers.addConnection")} ${index + 1}`}
                  </h3>
                  <small>{connection?.templateEndpointId ?? t("providers.customEndpoint")}</small>
                  {verification ? (
                    <Badge
                      tone={
                        verification.status === "valid"
                          ? "good"
                          : verification.status === "invalid"
                            ? "bad"
                            : "neutral"
                      }
                    >
                      {t(`status.${verification.status}`)}
                    </Badge>
                  ) : null}
                </div>
                {!selectedTemplate ? (
                  <Button
                    type="button"
                    variant="danger"
                    disabled={fields.fields.length === 1}
                    onClick={() => fields.remove(index)}
                  >
                    <Trash2 size={15} /> {t("common.delete")}
                  </Button>
                ) : null}
              </div>
              <div className="form-grid two-columns">
                <Field label={t("config.protocol")}>
                  {selectedTemplate ? (
                    <>
                      <input type="hidden" {...form.register(`connections.${index}.protocol`)} />
                      <Input value={connection?.protocol ?? ""} disabled />
                    </>
                  ) : (
                    <Select {...form.register(`connections.${index}.protocol`)}>
                      <option value="openai-chat">OpenAI Chat Completions</option>
                      <option value="openai-responses">OpenAI Responses</option>
                      <option value="anthropic-messages">Anthropic Messages</option>
                    </Select>
                  )}
                </Field>
                <Field label={t("providers.authType")}>
                  <Select {...form.register(`connections.${index}.authType`)}>
                    {(endpointTemplate?.authOptions ?? []).map((option) => (
                      <option key={option.id} value={option.authType}>
                        {option.authType === "api-key" ? "X-Api-Key" : "Bearer"}
                      </option>
                    ))}
                    {!endpointTemplate && connection?.protocol === "anthropic-messages" ? (
                      <option value="api-key">X-Api-Key</option>
                    ) : null}
                    {!endpointTemplate ? <option value="bearer">Bearer</option> : null}
                  </Select>
                </Field>
                <Field label={t("providers.endpoint")}>
                  <Input {...form.register(`connections.${index}.endpoint`)} />
                </Field>
                <Field label={t("providers.defaultModel")} hint={t("providers.modelHint")}>
                  <Input
                    list={`models-${index}`}
                    {...form.register(`connections.${index}.defaultModel`)}
                  />
                  <datalist id={`models-${index}`}>
                    {suggestions.map((model) => (
                      <option key={model} value={model} />
                    ))}
                  </datalist>
                </Field>
                {isFirstForSlot ? (
                  <Field
                    label={
                      selectedTemplate?.credentialSlots.find(
                        (slot) => slot.id === connection?.credentialSlotId,
                      )?.name ?? t("providers.key")
                    }
                    hint={
                      connections.filter(
                        (item) => item.credentialSlotId === connection?.credentialSlotId,
                      ).length > 1
                        ? t("providers.sharedCredential")
                        : t("providers.clipboardWarning")
                    }
                  >
                    <div className="input-action">
                      {(() => {
                        const registration = form.register(`connections.${index}.apiKey`);
                        return (
                          <Input
                            type="text"
                            autoComplete="off"
                            spellCheck={false}
                            {...registration}
                            onChange={(event) => {
                              registration.onChange(event);
                              connections.forEach((item, itemIndex) => {
                                if (
                                  itemIndex !== index &&
                                  item.credentialSlotId === connection?.credentialSlotId
                                ) {
                                  form.setValue(
                                    `connections.${itemIndex}.apiKey`,
                                    event.target.value,
                                    { shouldDirty: true },
                                  );
                                }
                              });
                            }}
                          />
                        );
                      })()}
                      <Button
                        type="button"
                        variant="ghost"
                        title={t("common.copy")}
                        onClick={async () => {
                          await navigator.clipboard.writeText(
                            form.getValues(`connections.${index}.apiKey`),
                          );
                          setNotice(t("common.copied"));
                        }}
                      >
                        <Copy size={15} />
                      </Button>
                    </div>
                  </Field>
                ) : null}
              </div>
              <div className="section-actions">
                <Button
                  type="button"
                  variant="secondary"
                  onClick={() => test(form.getValues(`connections.${index}.id`))}
                >
                  <Wifi size={15} /> {t("providers.test")}
                </Button>
                <Button
                  type="button"
                  variant="secondary"
                  onClick={() =>
                    loadModels(form.getValues(`connections.${index}.id`), index, field.id)
                  }
                >
                  {t("providers.fetchModels")}
                </Button>
              </div>
            </Card>
          );
        })}
      </div>
      {!selectedTemplate ? (
        <Button
          type="button"
          variant="secondary"
          onClick={() =>
            fields.append({
              credentialSlotId: `custom-api-key-${Date.now()}`,
              protocol: "openai-chat",
              endpoint: "https://api.example.com/v1",
              authType: "bearer",
              apiKey: "",
              defaultModel: "",
            })
          }
        >
          <Plus size={15} /> {t("providers.addConnection")}
        </Button>
      ) : null}
      {notice ? (
        <div className="notice" role="status">
          {notice}
        </div>
      ) : null}
      {Object.keys(form.formState.errors).length ? (
        <div className="diagnostic">{t("providers.invalidConnections")}</div>
      ) : null}
    </form>
  );
}
