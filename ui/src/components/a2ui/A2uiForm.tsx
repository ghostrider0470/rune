import { useMemo, useState } from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { A2uiComponent } from "@/lib/api-types";

interface Props {
  component: A2uiComponent;
  onSubmit?: (callbackId: string, data: Record<string, unknown>) => void | Promise<void>;
}

function normalizeInitialValue(field: Record<string, unknown>): string | boolean {
  const fieldType = String(field.field_type ?? "text");
  const defaultValue = field.default_value;
  if (fieldType === "checkbox") {
    return Boolean(defaultValue);
  }
  return defaultValue == null ? "" : String(defaultValue);
}

export function A2uiForm({ component, onSubmit }: Props) {
  const title = component.title != null ? String(component.title) : "Form";
  const callbackId = String(component.callback_id ?? component.id);
  const submitLabel = component.submit_label != null ? String(component.submit_label) : "Submit";
  const fields = Array.isArray(component.fields)
    ? component.fields.filter((field): field is Record<string, unknown> => !!field && typeof field === "object")
    : [];

  const initialValues = useMemo(
    () => Object.fromEntries(fields.map((field) => [String(field.key), normalizeInitialValue(field)])),
    [fields],
  );

  const [values, setValues] = useState<Record<string, string | boolean>>(initialValues);
  const [errors, setErrors] = useState<Record<string, string>>({});

  const handleSubmit = async () => {
    const nextErrors: Record<string, string> = {};
    const payload: Record<string, unknown> = {};

    for (const field of fields) {
      const key = String(field.key);
      const fieldType = String(field.field_type ?? "text");
      const required = Boolean(field.required);
      const value = values[key];

      if (required && (value === "" || value == null || value === false)) {
        nextErrors[key] = "Required";
        continue;
      }

      if (fieldType === "number") {
        payload[key] = value === "" ? null : Number(value);
      } else {
        payload[key] = value;
      }
    }

    setErrors(nextErrors);
    if (Object.keys(nextErrors).length > 0) return;
    await onSubmit?.(callbackId, payload);
  };

  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-sm">{title}</CardTitle>
      </CardHeader>
      <CardContent className="space-y-3">
        {fields.map((field) => {
          const key = String(field.key);
          const label = String(field.label ?? key);
          const fieldType = String(field.field_type ?? "text");
          const placeholder = field.placeholder != null ? String(field.placeholder) : undefined;
          const value = values[key];

          return (
            <label className="block space-y-1 text-sm" key={`${component.id}-${key}`}>
              <span className="font-medium">{label}</span>
              {fieldType === "textarea" ? (
                <Textarea
                  placeholder={placeholder}
                  value={String(value ?? "")}
                  onChange={(event) => setValues((current) => ({ ...current, [key]: event.target.value }))}
                />
              ) : fieldType === "select" ? (
                <Select
                  value={String(value ?? "")}
                  onValueChange={(next) => setValues((current) => ({ ...current, [key]: next }))}
                >
                  <SelectTrigger className="w-full">
                    <SelectValue placeholder={placeholder ?? "Select..."} />
                  </SelectTrigger>
                  <SelectContent>
                    {Array.isArray(field.options)
                      ? field.options.map((option, index) => {
                          const entry = option as Record<string, unknown>;
                          const optionValue = String(entry.value ?? index);
                          return (
                            <SelectItem key={`${component.id}-${key}-${optionValue}`} value={optionValue}>
                              {String(entry.label ?? optionValue)}
                            </SelectItem>
                          );
                        })
                      : null}
                  </SelectContent>
                </Select>
              ) : fieldType === "checkbox" ? (
                <input
                  checked={Boolean(value)}
                  className="h-4 w-4 rounded border"
                  onChange={(event) =>
                    setValues((current) => ({ ...current, [key]: event.target.checked }))
                  }
                  type="checkbox"
                />
              ) : (
                <Input
                  placeholder={placeholder}
                  type={fieldType === "number" ? "number" : "text"}
                  value={String(value ?? "")}
                  onChange={(event) => setValues((current) => ({ ...current, [key]: event.target.value }))}
                />
              )}
              {errors[key] ? <p className="text-xs text-destructive">{errors[key]}</p> : null}
            </label>
          );
        })}
        <Button onClick={() => void handleSubmit()}>{submitLabel}</Button>
      </CardContent>
    </Card>
  );
}
