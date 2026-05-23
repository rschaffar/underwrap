import { useState } from "react";
import { useMutation } from "@tanstack/react-query";

import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select } from "@/components/ui/select";
import {
  postPairCreate,
  postResourceCreate,
  type ResourceRecord,
  type WrapperResourcePairRecord,
} from "@/api/client";
import { describeApiError } from "@/lib/error-message";

export function CreatePairDialog({
  storeName,
  existingResources,
  existingPairs,
  onClose,
  onCreated,
}: {
  storeName: string;
  existingResources: ResourceRecord[];
  existingPairs: WrapperResourcePairRecord[];
  onClose: () => void;
  onCreated: () => void;
}) {
  const [wrapper, setWrapper] = useState("");
  // The selector tracks the backend resource id directly. The label
  // shown to the operator is resolved from `existingResources` (id →
  // name).
  const [resourceId, setResourceId] = useState<string>(
    existingResources[0]?.id ?? "",
  );
  const [creatingNewResource, setCreatingNewResource] = useState(
    existingResources.length === 0,
  );
  const [newResourceName, setNewResourceName] = useState("");
  const [submitError, setSubmitError] = useState<string | null>(null);

  const createResource = useMutation({
    mutationFn: (name: string) => postResourceCreate(storeName, { name }),
  });
  const createPair = useMutation({
    mutationFn: (vars: { wrapper: string; resource_id: string }) =>
      postPairCreate(storeName, {
        wrapper: vars.wrapper,
        resource_id: vars.resource_id,
      }),
  });

  async function onSave() {
    setSubmitError(null);
    const w = wrapper.trim();
    if (!w) {
      setSubmitError("Wrapper name is required.");
      return;
    }
    let chosenResourceId = resourceId;
    if (creatingNewResource) {
      const r = newResourceName.trim();
      if (!r) {
        setSubmitError("Resource name is required.");
        return;
      }
      try {
        const created = await createResource.mutateAsync(r);
        chosenResourceId = created.resource.id;
      } catch (e) {
        setSubmitError(describeApiError(e));
        return;
      }
    }
    if (!chosenResourceId) {
      setSubmitError("Pick or create a resource.");
      return;
    }
    if (
      existingPairs.some(
        (p) => p.wrapper === w && p.resource_id === chosenResourceId,
      )
    ) {
      setSubmitError(`Pair (${w}, ${chosenResourceId}) already exists.`);
      return;
    }
    try {
      await createPair.mutateAsync({ wrapper: w, resource_id: chosenResourceId });
      onCreated();
    } catch (e) {
      setSubmitError(describeApiError(e));
    }
  }

  const pending = createResource.isPending || createPair.isPending;

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Create wrapper/resource pair</DialogTitle>
          <DialogDescription>
            A wrapper/resource pair is the unit wrapper permissions
            reference. Each registered pair becomes a row in the table.
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-3">
          <div className="space-y-1.5">
            <Label htmlFor="pair-wrapper">Wrapper name</Label>
            <Input
              id="pair-wrapper"
              value={wrapper}
              onChange={(e) => setWrapper(e.target.value)}
              placeholder="e.g. gh-authorized"
              autoFocus
              autoComplete="off"
            />
          </div>
          <div className="space-y-1.5">
            <Label>Resource</Label>
            {existingResources.length > 0 ? (
              <div className="flex items-center gap-2 text-sm">
                <label className="inline-flex items-center gap-1.5">
                  <input
                    type="radio"
                    name="resource-mode"
                    checked={!creatingNewResource}
                    onChange={() => setCreatingNewResource(false)}
                  />
                  Pick existing
                </label>
                <label className="inline-flex items-center gap-1.5">
                  <input
                    type="radio"
                    name="resource-mode"
                    checked={creatingNewResource}
                    onChange={() => setCreatingNewResource(true)}
                  />
                  Create new
                </label>
              </div>
            ) : null}
            {creatingNewResource ? (
              <Input
                data-testid="pair-new-resource"
                value={newResourceName}
                onChange={(e) => setNewResourceName(e.target.value)}
                placeholder="e.g. github/token"
                autoComplete="off"
              />
            ) : (
              <Select
                data-testid="pair-resource-select"
                value={resourceId}
                onChange={(e) => setResourceId(e.target.value)}
                className="h-auto px-2 py-1.5"
              >
                {existingResources.map((r) => (
                  <option key={r.id} value={r.id}>
                    {r.name}
                  </option>
                ))}
              </Select>
            )}
          </div>
          {submitError ? <Alert>{submitError}</Alert> : null}
        </div>
        <DialogFooter>
          <Button type="button" variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button type="button" onClick={() => void onSave()} disabled={pending}>
            {pending ? "Creating…" : "Create"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
