import { useEffect, useRef, useState } from "react";
import { zodResolver } from "@hookform/resolvers/zod";
import { useFieldArray, useForm, useWatch } from "react-hook-form";
import { z } from "zod";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Copy, Files, Plus, Save, Trash2, Wifi } from "lucide-react";
import { useTranslation } from "react-i18next";
import { command, errorMessage } from "../../shared/ipc";
import { uniqueCopyName, validateEntityName } from "../../shared/names";
import type {
  ApiProviderDetail,
  ApiProviderDraft,
  CliProtocol,
  PublicProvider,
} from "../../shared/types";
import { useUiStore } from "../../stores/ui";
import { Badge, Button, Card, Field, Input, Select } from "../ui";

const connectionSchema = z.object({
  id: z.string().optional(),
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
    codingPlan: z.boolean(),
    codingPlanName: z.string().optional(),
    connections: z.array(connectionSchema).min(1),
  })
  .superRefine((value, context) => {
    const protocols = new Set<CliProtocol>();
    value.connections.forEach((connection, index) => {
      if (protocols.has(connection.protocol))
        context.addIssue({
          code: "custom",
          message: "Protocol must be unique",
          path: ["connections", index, "protocol"],
        });
      protocols.add(connection.protocol);
      if (connection.protocol !== "anthropic-messages" && connection.authType !== "bearer")
        context.addIssue({
          code: "custom",
          message: "OpenAI-compatible protocols require bearer authentication",
          path: ["connections", index, "authType"],
        });
    });
  });

export function ApiProviderEditor({
  detail,
  providers,
  onClose,
  onError,
}: {
  detail?: ApiProviderDetail;
  providers: PublicProvider[];
  onClose: () => void;
  onError: (message: string) => void;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const setDirty = useUiStore((state) => state.setDirty);
  const setSaveCurrent = useUiStore((state) => state.setSaveCurrent);
  const [notice, setNotice] = useState<string>();
  const [modelOptions, setModelOptions] = useState<Record<number, string[]>>({});
  const form = useForm<ApiProviderDraft>({
    resolver: zodResolver(schema),
    defaultValues: detail
      ? {
          name: detail.name,
          codingPlan: detail.codingPlan,
          codingPlanName: detail.codingPlanName ?? "",
          connections: detail.connections.map((connection) => ({
            ...connection,
            id: connection.id,
          })),
        }
      : {
          name: "",
          codingPlan: false,
          codingPlanName: "",
          connections: [
            {
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
  const codingPlan = useWatch({ control: form.control, name: "codingPlan" });
  const connections = useWatch({ control: form.control, name: "connections" });
  const providerName = useWatch({ control: form.control, name: "name" });
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
  useEffect(() => {
    setDirty(form.formState.isDirty);
  }, [form.formState.isDirty, setDirty]);
  const saveCurrentRef = useRef<() => Promise<boolean>>(async () => false);
  useEffect(() => {
    saveCurrentRef.current = async () => {
      if (!(await form.trigger()) || nameIssue) return false;
      try {
        const value = form.getValues();
        await save.mutateAsync({ ...value, name: value.name.trim() });
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
      const value = form.getValues();
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
  const models = async (connectionId: string | undefined, index: number) => {
    if (!detail || !connectionId) {
      setNotice(t("providers.saveBeforeModels"));
      return;
    }
    try {
      const values = await command<string[]>("list_models", {
        providerId: detail.id,
        connectionId,
      });
      setModelOptions((current) => ({ ...current, [index]: values }));
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
        if (!nameIssue) save.mutate({ ...value, name: value.name.trim() });
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
        <label className="switch-row">
          <input type="checkbox" {...form.register("codingPlan")} />
          {t("providers.codingPlan")}
        </label>
        {codingPlan ? (
          <Field label={t("providers.planName")}>
            <Input {...form.register("codingPlanName")} />
          </Field>
        ) : null}
      </div>
      <div className="connection-list">
        {fields.fields.map((field, index) => {
          const verification = detail?.connections.find(
            (connection) => connection.id === form.getValues(`connections.${index}.id`),
          )?.verification;
          return (
            <Card key={field.id}>
              <div className="card-title-row">
                <div>
                  <h3>
                    {t("providers.addConnection")} {index + 1}
                  </h3>
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
                <Button
                  type="button"
                  variant="danger"
                  disabled={fields.fields.length === 1}
                  onClick={() => fields.remove(index)}
                >
                  <Trash2 size={15} /> {t("common.delete")}
                </Button>
              </div>
              <div className="form-grid two-columns">
                <Field label={t("config.protocol")}>
                  <Select {...form.register(`connections.${index}.protocol`)}>
                    <option value="openai-chat">OpenAI Chat Completions</option>
                    <option value="openai-responses">OpenAI Responses</option>
                    <option value="anthropic-messages">Anthropic Messages</option>
                  </Select>
                </Field>
                <Field label={t("providers.authType")}>
                  <Select {...form.register(`connections.${index}.authType`)}>
                    {connections[index]?.protocol === "anthropic-messages" ? (
                      <option value="api-key">X-Api-Key</option>
                    ) : null}
                    <option value="bearer">Bearer</option>
                  </Select>
                </Field>
                <Field label={t("providers.endpoint")}>
                  <Input {...form.register(`connections.${index}.endpoint`)} />
                </Field>
                <Field label={t("providers.defaultModel")}>
                  <Input
                    list={`models-${index}`}
                    {...form.register(`connections.${index}.defaultModel`)}
                  />
                  <datalist id={`models-${index}`}>
                    {modelOptions[index]?.map((model) => (
                      <option key={model} value={model} />
                    ))}
                  </datalist>
                </Field>
                <Field label={t("providers.key")} hint={t("providers.clipboardWarning")}>
                  <div className="input-action">
                    <Input
                      type="text"
                      autoComplete="off"
                      spellCheck={false}
                      {...form.register(`connections.${index}.apiKey`)}
                    />
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
                  onClick={() => models(form.getValues(`connections.${index}.id`), index)}
                >
                  {t("providers.fetchModels")}
                </Button>
              </div>
            </Card>
          );
        })}
      </div>
      <Button
        type="button"
        variant="secondary"
        disabled={fields.fields.length >= 3}
        onClick={() =>
          fields.append({
            protocol:
              (["openai-chat", "openai-responses", "anthropic-messages"].find(
                (protocol) =>
                  !form.getValues("connections").some((item) => item.protocol === protocol),
              ) as CliProtocol) ?? "openai-chat",
            endpoint: "https://api.example.com/v1",
            authType: "bearer",
            apiKey: "",
            defaultModel: "",
          })
        }
      >
        <Plus size={15} /> {t("providers.addConnection")}
      </Button>
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
