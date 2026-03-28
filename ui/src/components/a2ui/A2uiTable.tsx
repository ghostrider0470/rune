import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import type { A2uiComponent } from "@/lib/api-types";

function renderCell(value: unknown, columnType: string) {
  if (columnType === "badge") {
    return <Badge variant="outline">{String(value ?? "")}</Badge>;
  }

  if (columnType === "code") {
    return <code className="rounded bg-muted px-1.5 py-0.5 text-xs">{String(value ?? "")}</code>;
  }

  if (columnType === "date" && typeof value === "string") {
    return new Date(value).toLocaleString();
  }

  return String(value ?? "");
}

export function A2uiTable({ component }: { component: A2uiComponent }) {
  const title = component.title != null ? String(component.title) : null;
  const columns = Array.isArray(component.columns) ? component.columns : [];
  const rows = Array.isArray(component.rows) ? component.rows.slice(0, 50) : [];
  const showRowNumbers = Boolean(component.show_row_numbers);

  return (
    <Card>
      {title ? (
        <CardHeader className="pb-2">
          <CardTitle className="text-sm">{title}</CardTitle>
        </CardHeader>
      ) : null}
      <CardContent className={title ? "" : "pt-4"}>
        <div className="rounded-md border">
          <Table>
            <TableHeader>
              <TableRow>
                {showRowNumbers ? <TableHead className="w-12">#</TableHead> : null}
                {columns.map((column, index) => (
                  <TableHead key={`${String((column as { key?: unknown }).key ?? index)}`}>
                    {String((column as { label?: unknown }).label ?? (column as { key?: unknown }).key ?? index)}
                  </TableHead>
                ))}
              </TableRow>
            </TableHeader>
            <TableBody>
              {rows.length === 0 ? (
                <TableRow>
                  <TableCell className="text-muted-foreground" colSpan={columns.length + (showRowNumbers ? 1 : 0)}>
                    No data
                  </TableCell>
                </TableRow>
              ) : (
                rows.map((row, rowIndex) => {
                  const record = row && typeof row === "object" ? (row as Record<string, unknown>) : {};
                  return (
                    <TableRow key={`${component.id}-${rowIndex}`}>
                      {showRowNumbers ? <TableCell>{rowIndex + 1}</TableCell> : null}
                      {columns.map((column, columnIndex) => {
                        const key = String((column as { key?: unknown }).key ?? columnIndex);
                        const colType = String((column as { col_type?: unknown }).col_type ?? "text");
                        return (
                          <TableCell key={`${component.id}-${rowIndex}-${key}`}>
                            {renderCell(record[key], colType)}
                          </TableCell>
                        );
                      })}
                    </TableRow>
                  );
                })
              )}
            </TableBody>
          </Table>
        </div>
      </CardContent>
    </Card>
  );
}
